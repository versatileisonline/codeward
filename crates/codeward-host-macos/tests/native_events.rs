fn main() {
    #[cfg(target_os = "macos")]
    macos::run();
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{
        ffi::{CStr, c_char, c_void},
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
        println!(
            "macOS main-thread registration, local notification delivery and control revocation passed"
        );
    }
}
