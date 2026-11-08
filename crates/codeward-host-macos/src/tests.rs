use super::*;

#[test]
fn native_wait_preserves_short_deadlines_and_rechecks_idle_sleep_gaps() {
    assert_eq!(wait_seconds(Duration::ZERO), 0.0);
    assert_eq!(wait_seconds(Duration::from_millis(20)), 0.02);
    assert_eq!(wait_seconds(Duration::from_secs(300)), 1.0);
}

#[test]
fn sleep_gap_uses_brackets_and_detects_a_gap_without_notifications() {
    let mut gap = SleepGap::new([100, 101, 103]).unwrap();
    assert!(!gap.observe([200, 202, 204]).unwrap());
    assert!(gap.observe([300, 351, 302]).unwrap());
    assert!(!gap.observe([400, 451, 402]).unwrap());
}

#[test]
fn invalid_or_regressing_clock_observations_fail_closed() {
    assert!(SleepGap::new([20, 21, 19]).is_err());
    let mut gap = SleepGap::new([100, 151, 102]).unwrap();
    assert!(gap.observe([101, 160, 110]).is_err());
    assert!(gap.observe([200, 149, 202]).is_err());
    assert!(gap.observe([200, 201, 202]).is_err());
}

#[test]
fn screen_and_workspace_observations_are_revocation_only() {
    assert!(matches!(
        observation(1),
        Some(SessionEvent::OsSessionLocked)
    ));
    assert!(matches!(
        observation(2),
        Some(SessionEvent::SessionSwitchedAway)
    ));
    assert!(matches!(observation(4), Some(SessionEvent::Suspend)));
    assert!(matches!(observation(8), Some(SessionEvent::Resume)));
    assert!(matches!(observation(16), Some(SessionEvent::Reconnected)));
    assert!(matches!(
        observation(1 | 16),
        Some(SessionEvent::OsSessionLocked)
    ));
    assert!(observation(0).is_none());
}
