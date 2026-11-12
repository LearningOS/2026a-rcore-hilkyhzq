extern crate alloc;
#[path = "../os/src/sync/deadlock.rs"]
mod rcore_deadlock;
use rcore_deadlock::DeadlockState;
const A: (u8, usize) = (0, 0);
const B: (u8, usize) = (0, 1);
#[test]
fn self_deadlock_is_rejected_and_rolled_back() {
    let mut s = DeadlockState::new();
    s.set_enabled(true);
    s.register(A, 1, true);
    assert!(s.request(A, 1));
    assert!(!s.request(A, 1));
    assert!(s.release(A, 1, true));
    assert!(s.request(A, 2));
}
#[test]
fn cycle_across_two_resources_is_rejected() {
    let mut s = DeadlockState::new();
    s.set_enabled(true);
    s.register(A, 1, true);
    s.register(B, 1, true);
    assert!(s.request(A, 1));
    assert!(s.request(B, 2));
    assert!(s.request(B, 1));
    assert!(!s.request(A, 2));
    assert!(s.release(B, 2, true));
    s.acquired(B, 1);
    assert!(!s.request(B, 1));
}
#[test]
fn safe_wait_with_an_unblocked_holder_is_allowed() {
    let mut s = DeadlockState::new();
    s.set_enabled(true);
    s.register(A, 2, true);
    assert!(s.request(A, 1));
    assert!(s.request(A, 2));
    assert!(s.request(A, 3));
    assert!(s.release(A, 2, true));
    s.acquired(A, 3);
    assert!(s.release(A, 3, true));
    assert!(s.release(A, 1, true));
}
#[test]
fn blocking_handoff_is_reserved_before_waiter_resumes() {
    let mut s = DeadlockState::new();
    s.set_enabled(true);
    s.register(A, 1, true);
    assert!(s.request(A, 1));
    assert!(s.request(A, 2));
    assert!(s.release(A, 1, true));
    assert!(s.request(A, 3));
    s.acquired(A, 2);
    assert!(!s.request(A, 2));
}
#[test]
fn spin_waiter_can_win_out_of_fifo_order() {
    let mut s = DeadlockState::new();
    s.set_enabled(true);
    s.register(A, 1, false);
    assert!(s.request(A, 1));
    assert!(s.request(A, 2));
    assert!(s.request(A, 3));
    assert!(s.release(A, 1, true));
    s.acquired(A, 3);
    assert!(!s.request(A, 3));
    assert!(s.release(A, 3, true));
    s.acquired(A, 2);
    assert!(s.release(A, 2, true));
}
#[test]
fn retired_owner_does_not_magically_release_its_resource() {
    let mut s = DeadlockState::new();
    s.set_enabled(true);
    s.register(A, 1, true);
    assert!(s.request(A, 1));
    s.retire(1);
    assert!(!s.request(A, 2));
}
#[test]
fn mutex_unlock_requires_ownership() {
    let mut s = DeadlockState::new();
    s.register(A, 1, true);
    assert!(s.request(A, 1));
    assert!(!s.release(A, 2, true));
    assert!(s.release(A, 1, true));
}
#[test]
fn disabled_detection_keeps_bookkeeping_for_later_enable() {
    let mut s = DeadlockState::new();
    s.register(A, 1, true);
    assert!(s.request(A, 1));
    s.set_enabled(true);
    assert!(!s.request(A, 1));
    s.set_enabled(false);
    assert!(s.request(A, 1));
}
