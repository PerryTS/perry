//! #9877: the diagnostic must describe the actual repair walk and its slots.
use super::super::*;
use super::support::*;

fn exercise_restore() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_remembered_set();

    unsafe {
        // Two walked parents have four slots, but only two young edges.
        // The third parent's primitive slot is already covered by the scan.
        let (one, one_slot) = alloc_old_test_object(1);
        let (three, three_slots) = alloc_old_test_object(3);
        let (skipped, skipped_slot) = alloc_old_test_object(1);
        let (young, _) = alloc_nursery_test_object(0);
        assert!(!crate::arena::pointer_in_old_gen(young as usize));
        let young_bits = POINTER_TAG | (young as u64 & POINTER_MASK);
        *one_slot = young_bits;
        *three_slots = young_bits;
        *three_slots.add(1) = POINTER_TAG | (one as u64 & POINTER_MASK);
        *three_slots.add(2) = 42.0f64.to_bits();
        *skipped_slot = 7.0f64.to_bits();

        let page = crate::arena::generation_page_for_addr(one as usize);
        assert_eq!(page, crate::arena::generation_page_for_addr(three as usize));
        assert_eq!(
            page,
            crate::arena::generation_page_for_addr(skipped as usize)
        );
        let snapshot = RememberedDirtySnapshot {
            dirty_old_pages: [page].into_iter().collect(),
            // A repeated stale external owner is deduplicated, then rejected
            // without dereferencing it. These pages are not old walk inputs.
            external_dirty_entries: vec![(page + 1, 0), (page + 2, 0)],
            dirty_pages: [page, page + 1, page + 2].into_iter().collect(),
            fallback_headers: Vec::new(),
        };
        let covered = [header_from_user_ptr(skipped as *const u8) as usize]
            .into_iter()
            .collect();
        remembered_set_clear();
        restore_surviving_dirty_coverage(&snapshot, &covered, "first");
        assert!(
            barrier::DIRTY_OLD_PAGES.with(|s| s.borrow().contains(&page)),
            "repair must restore the page containing the young edges"
        );
        // Productivity counts edges, even when their page is already dirty.
        restore_surviving_dirty_coverage(&snapshot, &covered, "repeat");
        assert_eq!(barrier::DIRTY_OLD_PAGES.with(|s| s.borrow().len()), 1);
    }
    remembered_set_clear();
    println!("restore coverage witness completed");
}

fn field(line: &str, name: &str) -> usize {
    let prefix = format!("{name}=");
    line.split_whitespace()
        .find_map(|word| word.strip_prefix(&prefix))
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("missing numeric {name} in {line}"))
}

#[test]
fn restore_coverage_diagnostic_matches_walk_and_off_arm() {
    const CHILD: &str = "PERRY_TEST_RESTORE_COVERAGE_CHILD";
    let thread = std::thread::current();
    let name = thread.name().expect("libtest thread name");
    if std::env::var(CHILD).ok().as_deref() == Some(name) {
        exercise_restore();
        return;
    }
    for enabled in ["0", "1"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture", "--test-threads=1"])
            .env(CHILD, name)
            .env("PERRY_GC_DIAG", enabled)
            .output()
            .expect("run isolated diagnostic witness");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stdout.contains("restore coverage witness completed"),
            "witness failed (diag={enabled}): {}\n{stdout}\n{stderr}",
            output.status
        );
        let lines: Vec<_> = stderr
            .lines()
            .filter(|line| line.starts_with("[gc-restore-coverage] "))
            .collect();
        if enabled == "0" {
            assert!(
                lines.is_empty(),
                "diagnostics off must stay silent: {stderr}"
            );
            continue;
        }
        assert_eq!(lines.len(), 2, "one diagnostic per repair: {stderr}");
        for (line, added) in lines.iter().zip([1, 0]) {
            assert_eq!(field(line, "dirty_old_pages"), 1);
            assert_eq!(field(line, "external_entries"), 2);
            assert_eq!(field(line, "covered"), 1);
            assert_eq!(field(line, "objects_walked"), 3);
            assert_eq!(field(line, "objects_skipped"), 1);
            assert_eq!(field(line, "parents_visited"), 2);
            assert_eq!(field(line, "slots_visited"), 4);
            assert_eq!(field(line, "slots_tracking"), 2);
            assert_eq!(field(line, "pages_added"), added);
            assert!(
                !line.contains(" dirty_pages="),
                "do not mislabel the walk input"
            );
        }
    }
}
