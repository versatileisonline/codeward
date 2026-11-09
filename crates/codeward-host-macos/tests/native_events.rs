fn main() {
    #[cfg(target_os = "macos")]
    match std::env::args().nth(1).as_deref() {
        Some("--live-lock") => macos::live::run(macos::live::Scenario::Lock),
        Some("--live-sleep") => macos::live::run(macos::live::Scenario::Sleep),
        _ => macos::run(),
    }
    #[cfg(not(target_os = "macos"))]
    if matches!(
        std::env::args().nth(1).as_deref(),
        Some("--live-lock" | "--live-sleep")
    ) {
        panic!("live session-event probes require macOS");
    }
}

#[cfg(target_os = "macos")]
mod macos {
    #[path = "live.rs"]
    pub(super) mod live;

    use std::{
        ffi::{CStr, c_char, c_void},
        ptr,
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };

    use codeward_core::transaction::{
        CallerId, ControlError, HeadlessControlLoop, HeadlessEventSource, HeadlessVault,
        PublishError, PublishMode, SessionEvent, Storage, StorageError, WriterLease,
    };
    use codeward_host_macos::MacosEventSource;

    type Object = *mut c_void;
    type Selector = *const c_void;

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Object;
        fn sel_registerName(name: *const c_char) -> Selector;
        fn objc_msgSend();
        static NSWorkspaceWillSleepNotification: Object;
        static NSWorkspaceDidWakeNotification: Object;
        static NSWorkspaceSessionDidResignActiveNotification: Object;
        static NSWorkspaceSessionDidBecomeActiveNotification: Object;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopDefaultMode: Object;
        fn CFRunLoopGetCurrent() -> Object;
        fn CFRunLoopObserverCreate(
            allocator: *const c_void,
            activities: usize,
            repeats: u8,
            order: isize,
            callback: extern "C" fn(Object, usize, *mut c_void),
            context: *mut c_void,
        ) -> Object;
        fn CFRunLoopAddObserver(run_loop: Object, observer: Object, mode: Object);
        fn CFRunLoopObserverInvalidate(observer: Object);
        fn CFRunLoopRunInMode(mode: Object, seconds: f64, return_after_source: u8) -> i32;
        fn CFRelease(object: Object);
    }

    static LOOP_ENTERED: AtomicBool = AtomicBool::new(false);

    extern "C" fn record_loop_entry(_: Object, _: usize, _: *mut c_void) {
        LOOP_ENTERED.store(true, Ordering::Relaxed);
    }

    fn post_workspace_notification(name: Object) {
        // SAFETY: run on the main thread. These exact SDK selectors return the
        // shared workspace and its center, then post only inside this process.
        // The two message sends use the Objective-C ABI for their signatures.
        unsafe {
            let get: unsafe extern "C" fn(Object, Selector) -> Object =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let post: unsafe extern "C" fn(Object, Selector, Object, Object) =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let class = objc_getClass(c"NSWorkspace".as_ptr());
            let workspace = get(class, selector(c"sharedWorkspace"));
            let center = get(workspace, selector(c"notificationCenter"));
            assert!(!workspace.is_null() && !center.is_null());
            post(
                center,
                selector(c"postNotificationName:object:"),
                name,
                workspace,
            );
        }
    }

    fn selector(name: &CStr) -> Selector {
        // SAFETY: name is a live NUL-terminated selector string.
        unsafe { sel_registerName(name.as_ptr()) }
    }

    struct Absent;

    impl Storage for Absent {
        type Lease = Self;
        fn acquire(self) -> Result<Self, StorageError> {
            Ok(self)
        }
    }

    impl WriterLease for Absent {
        fn read_current(&mut self) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(None)
        }
        fn stage(&mut self, _: &[u8]) -> Result<(), StorageError> {
            unreachable!()
        }
        fn publish(&mut self, _: PublishMode) -> Result<(), PublishError> {
            unreachable!()
        }
        fn persist(&mut self) -> Result<(), StorageError> {
            unreachable!()
        }
    }

    pub fn run() {
        assert!(
            std::thread::spawn(|| MacosEventSource::new().is_err())
                .join()
                .unwrap()
        );
        let mut source = MacosEventSource::new().expect("main-thread macOS registration");
        source.take_revocation().unwrap();
        // SAFETY: the linked AppKit constants are live notification names.
        let notifications = unsafe {
            [
                NSWorkspaceWillSleepNotification,
                NSWorkspaceDidWakeNotification,
                NSWorkspaceSessionDidResignActiveNotification,
                NSWorkspaceSessionDidBecomeActiveNotification,
            ]
        };
        for (name, expected) in notifications.into_iter().zip([
            SessionEvent::Suspend,
            SessionEvent::Resume,
            SessionEvent::SessionSwitchedAway,
            SessionEvent::Reconnected,
        ]) {
            post_workspace_notification(name);
            source.wait(Duration::from_millis(10)).unwrap();
            assert_eq!(source.take_revocation().unwrap(), Some(expected));
        }
        let host = HeadlessVault::begin_create(Absent, CallerId(1)).unwrap();
        let mut control = HeadlessControlLoop::new(host, source);
        assert!(control.command(|host| host.recovery_code()).is_ok());
        post_workspace_notification(notifications[0]);
        assert!(matches!(
            control.command(|host| host.recovery_code()),
            Err(ControlError::Revoked)
        ));
        assert!(control.command(|host| Ok(host.is_locked())).unwrap());
        control.close();
        desktop_drive(notifications[0], notifications[1]);
        println!(
            "macOS main-thread registration, headless and desktop-driven notification revocation passed"
        );
    }

    fn desktop_drive(suspend: Object, resume: Object) {
        let source = MacosEventSource::for_desktop_loop().unwrap();
        let host = HeadlessVault::begin_create(Absent, CallerId(1)).unwrap();
        let mut control = HeadlessControlLoop::new(host, source);
        // SAFETY: use the live main run loop, kCFRunLoopEntry (1), and a callback
        // with no borrowed context. The observer is invalidated and released below.
        let observer = unsafe {
            let observer =
                CFRunLoopObserverCreate(ptr::null(), 1, 1, 0, record_loop_entry, ptr::null_mut());
            assert!(!observer.is_null());
            CFRunLoopAddObserver(CFRunLoopGetCurrent(), observer, kCFRunLoopDefaultMode);
            observer
        };

        assert_eq!(
            control.next_poll_in(Duration::from_secs(60)),
            Duration::from_secs(1)
        );
        assert!(control.command(|host| host.recovery_code()).is_ok());
        assert!(matches!(
            control.command(|host| {
                let code = host.recovery_code()?;
                post_workspace_notification(suspend);
                Ok(code)
            }),
            Err(ControlError::Revoked)
        ));
        post_workspace_notification(resume);
        assert!(control.step(Duration::ZERO).unwrap().revoked);
        assert!(control.command(|host| Ok(host.is_locked())).unwrap());
        assert!(!LOOP_ENTERED.load(Ordering::Relaxed));

        // SAFETY: drive the main run loop from outside the vault boundary.
        unsafe {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.001, 0);
            CFRunLoopObserverInvalidate(observer);
            CFRelease(observer);
        }
        assert!(LOOP_ENTERED.load(Ordering::Relaxed));

        control.command(|host| host.restart_enrollment()).unwrap();
        let update = control.step(Duration::from_millis(1)).unwrap();
        assert!(update.revoked && !update.source_available);
        assert!(matches!(
            control.command(|host| host.recovery_code()),
            Err(ControlError::EventSourceUnavailable)
        ));
        control.close();
    }
}
