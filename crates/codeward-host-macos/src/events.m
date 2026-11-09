#import <AppKit/AppKit.h>
#import <CoreFoundation/CoreFoundation.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>

enum CWEvent {
    CWScreenLock = 1 << 0,
    CWSwitchAway = 1 << 1,
    CWSuspend = 1 << 2,
    CWResume = 1 << 3,
    CWReconnect = 1 << 4,
};

static void CWSourceReady(void *context) { (void)context; }

@interface CWSessionEvents : NSObject {
@public
    _Atomic(uint32_t) pending;
@private
    NSNotificationCenter *workspaceCenter;
    NSDistributedNotificationCenter *distributedCenter;
    CFRunLoopRef runLoop;
    CFRunLoopSourceRef wakeSource;
}
@end

@implementation CWSessionEvents

- (instancetype)init {
    self = [super init];
    if (!self) return nil;
    atomic_init(&pending, 0);
    runLoop = CFRunLoopGetCurrent();
    CFRetain(runLoop);
    CFRunLoopSourceContext context = {0};
    context.perform = CWSourceReady;
    wakeSource = CFRunLoopSourceCreate(kCFAllocatorDefault, 0, &context);
    if (!wakeSource) return nil;
    CFRunLoopAddSource(runLoop, wakeSource, kCFRunLoopDefaultMode);

    NSWorkspace *workspace = NSWorkspace.sharedWorkspace;
    workspaceCenter = workspace.notificationCenter;
    distributedCenter = NSDistributedNotificationCenter.defaultCenter;
    if (!workspaceCenter || !distributedCenter) return nil;
    [workspaceCenter addObserver:self selector:@selector(switchAway:) name:NSWorkspaceSessionDidResignActiveNotification object:nil];
    [workspaceCenter addObserver:self selector:@selector(reconnect:) name:NSWorkspaceSessionDidBecomeActiveNotification object:nil];
    [workspaceCenter addObserver:self selector:@selector(suspend:) name:NSWorkspaceWillSleepNotification object:nil];
    [workspaceCenter addObserver:self selector:@selector(resume:) name:NSWorkspaceDidWakeNotification object:nil];
    // These undocumented distributed names are untrusted revocation hints.
    // Ignore object/payload. Neither hint can install or refresh access.
    [distributedCenter addObserver:self selector:@selector(screenLock:) name:@"com.apple.screenIsLocked" object:nil suspensionBehavior:NSNotificationSuspensionBehaviorDeliverImmediately];
    [distributedCenter addObserver:self selector:@selector(reconnect:) name:@"com.apple.screenIsUnlocked" object:nil suspensionBehavior:NSNotificationSuspensionBehaviorDeliverImmediately];
    return self;
}

- (void)record:(enum CWEvent)event {
    atomic_fetch_or_explicit(&pending, (uint32_t)event, memory_order_release);
    CFRunLoopSourceSignal(wakeSource);
    CFRunLoopWakeUp(runLoop);
}

- (void)screenLock:(NSNotification *)note { (void)note; [self record:CWScreenLock]; }
- (void)switchAway:(NSNotification *)note { (void)note; [self record:CWSwitchAway]; }
- (void)suspend:(NSNotification *)note { (void)note; [self record:CWSuspend]; }
- (void)resume:(NSNotification *)note { (void)note; [self record:CWResume]; }
- (void)reconnect:(NSNotification *)note { (void)note; [self record:CWReconnect]; }

- (void)dealloc {
    [distributedCenter removeObserver:self];
    [workspaceCenter removeObserver:self];
    if (wakeSource) {
        CFRunLoopSourceInvalidate(wakeSource);
        CFRelease(wakeSource);
    }
    if (runLoop) CFRelease(runLoop);
}
@end

void *cw_macos_events_open(void) {
    @autoreleasepool {
        @try {
            if (!NSThread.isMainThread) return NULL;
            return (__bridge_retained void *)[[CWSessionEvents alloc] init];
        } @catch (NSException *exception) {
            (void)exception;
            return NULL;
        }
    }
}

void cw_macos_events_close(void *opaque) {
    @autoreleasepool {
        @try {
            // Consume the ownership transferred by cw_macos_events_open.
            (void)CFBridgingRelease(opaque);
        } @catch (NSException *exception) {
            (void)exception;
        }
    }
}

bool cw_macos_events_collect(void *opaque, bool pump, uint32_t *events) {
    @autoreleasepool {
        @try {
            CWSessionEvents *source = (__bridge CWSessionEvents *)opaque;
            // A zero-duration pass handles one source at most. Bound draining
            // so a continually busy native queue cannot delay revocation forever.
            for (unsigned attempt = 0; attempt < 32; attempt++) {
                *events = atomic_exchange_explicit(&source->pending, 0, memory_order_acq_rel);
                if (*events || !pump) return true;
                SInt32 status = CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0, true);
                if (status == kCFRunLoopRunTimedOut) {
                    *events = atomic_exchange_explicit(&source->pending, 0, memory_order_acq_rel);
                    return true;
                }
                if (status != kCFRunLoopRunHandledSource) return false;
            }
        } @catch (NSException *exception) {
            (void)exception;
        }
        return false;
    }
}

bool cw_macos_events_wait(double seconds) {
    @autoreleasepool {
        @try {
            SInt32 status = CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, true);
            return status == kCFRunLoopRunHandledSource || status == kCFRunLoopRunTimedOut;
        } @catch (NSException *exception) {
            (void)exception;
            return false;
        }
    }
}
