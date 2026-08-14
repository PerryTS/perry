use super::drain_expired_timers;

/// The single-pass partition must preserve the original order of BOTH
/// halves: expired entries fire in creation order (same-deadline Node
/// semantics) and survivors keep queue order for the next tick.
#[test]
fn timer_drain_partition_preserves_order() {
    // (id, cleared, expired)
    let mut queue = vec![
        (1, false, true),
        (2, false, false),
        (3, true, true),
        (4, false, true),
        (5, false, false),
        (6, true, false),
        (7, false, true),
    ];
    let expired = drain_expired_timers(&mut queue, |t| t.1, |t| t.2);
    let expired_ids: Vec<i32> = expired.iter().map(|t| t.0).collect();
    let kept_ids: Vec<i32> = queue.iter().map(|t| t.0).collect();
    assert_eq!(expired_ids, vec![1, 4, 7], "expired keep creation order");
    assert_eq!(kept_ids, vec![2, 5], "survivors keep queue order");
}

#[test]
fn timer_drain_partition_empty_and_all_expired() {
    let mut empty: Vec<i32> = Vec::new();
    assert!(drain_expired_timers(&mut empty, |_| false, |_| true).is_empty());

    let mut queue = vec![10, 20, 30];
    let expired = drain_expired_timers(&mut queue, |_| false, |_| true);
    assert_eq!(expired, vec![10, 20, 30]);
    assert!(queue.is_empty());
}
