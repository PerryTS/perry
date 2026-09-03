//! `PERRY_GC_CENSUS` (gc/census.rs): the instrument is validated against a
//! heap of KNOWN composition before it is pointed at anything real — a
//! census that has never been checked against a known answer is a random
//! number generator with a JSON schema.

use super::super::*;
use super::support::*;

fn take(label: &'static str) {
    super::super::census::census_arm(label);
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual));
}

fn read_lines(path: &str) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .expect("census file exists")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("census line is JSON"))
        .collect()
}

fn type_live(c: &serde_json::Value, name: &str) -> (u64, u64) {
    for t in c["by_type"].as_array().expect("by_type") {
        if t["type"] == name {
            return (
                t["live"]["count"].as_u64().unwrap_or(0),
                t["live"]["bytes"].as_u64().unwrap_or(0),
            );
        }
    }
    (0, 0)
}

fn class_row<'a>(c: &'a serde_json::Value, class_id: u64) -> Option<&'a serde_json::Value> {
    c["objects"]["by_class"]
        .as_array()
        .expect("by_class")
        .iter()
        .find(|r| r["class_id"] == class_id)
}

#[test]
fn census_reports_a_known_composition_and_sees_deadness() {
    std::thread::spawn(|| {
        let _copying = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        reset_global_roots();
        let _root_reset = ShadowAndGlobalRootResetGuard;

        let path = std::env::temp_dir().join(format!(
            "perry-gc-census-test-{}-{:?}.jsonl",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        let path_str = path.to_string_lossy().into_owned();
        super::super::census::test_set_census_path(Some(path_str.clone()));

        take("baseline");

        // Known composition: 1000 strings of 100 bytes + 500 objects with 8
        // inline slots, every one rooted through a registered global slot.
        const STRINGS: usize = 1000;
        const OBJECTS: usize = 500;
        let payload = [b'q'; 100];
        let mut roots: Vec<u64> = Vec::with_capacity(STRINGS + OBJECTS);
        for _ in 0..STRINGS {
            let s = crate::string::js_string_from_bytes(payload.as_ptr(), payload.len() as u32);
            roots.push(string_bits(s as usize));
        }
        for _ in 0..OBJECTS {
            let o = crate::object::js_object_alloc(0, 8);
            roots.push(crate::value::JSValue::pointer(o as *const u8).bits());
        }
        for slot in roots.iter_mut() {
            js_gc_register_global_root(slot as *mut u64 as i64);
        }

        take("populated");

        // Negative control: drop every root; the next census must see the
        // same objects as DEAD (they are still in the walked blocks).
        reset_global_roots();
        take("dropped");

        super::super::census::test_set_census_path(None);
        let lines = read_lines(&path_str);
        let _ = std::fs::remove_file(&path);
        assert_eq!(lines.len(), 3, "one census line per armed full collection");
        let (b, a, d) = (&lines[0], &lines[1], &lines[2]);
        assert_eq!(b["label"], "baseline");
        assert_eq!(a["label"], "populated");

        let (sb, sbb) = type_live(b, "string");
        let (sa, sab) = type_live(a, "string");
        let ds = sa as i64 - sb as i64;
        assert!(
            (STRINGS as i64..=STRINGS as i64 + 8).contains(&ds),
            "live string delta must be the {STRINGS} rooted strings (got {ds})"
        );
        let per_string = (sab - sbb) as f64 / STRINGS as f64;
        // GcHeader(8) + StringHeader(20) + 100 payload = 128, allocator rounding
        // may add a word.
        assert!(
            (128.0..=144.0).contains(&per_string),
            "per-string header-inclusive bytes off: {per_string}"
        );

        let ob = class_row(b, 0).map(|r| r["count"].as_u64().unwrap()).unwrap_or(0);
        let oa_row = class_row(a, 0).expect("class 0 row after allocation");
        let oa = oa_row["count"].as_u64().unwrap();
        assert_eq!(oa - ob, OBJECTS as u64, "500 rooted 8-slot objects");
        let cap_b = class_row(b, 0)
            .map(|r| r["slot_capacity"].as_u64().unwrap())
            .unwrap_or(0);
        let cap_a = oa_row["slot_capacity"].as_u64().unwrap();
        assert_eq!(cap_a - cap_b, 8 * OBJECTS as u64, "8 inline slots per object");

        // Deadness: the dropped population shows up as dead, not live.
        let (sd, _) = type_live(d, "string");
        assert!(
            (sd as i64 - sb as i64).abs() <= 8,
            "after dropping the roots the live string count returns to baseline ({sb} -> {sd})"
        );
        assert_eq!(d["totals"]["reachability_pass"], true);
        let dead = d["totals"]["dead_objects"].as_u64().unwrap();
        let late = d["totals"]["late_marked_objects"].as_u64().unwrap();
        assert!(
            dead + late >= (STRINGS + OBJECTS) as u64,
            "the dropped population must be dead or late-marked, never live (dead={dead} late={late})"
        );
        let (oa_after, _) = (class_row(d, 0).map(|r| r["count"].as_u64().unwrap()).unwrap_or(0), 0);
        assert!(
            oa_after <= ob + 8,
            "dropped objects must not count as live ({ob} baseline -> {oa_after})"
        );
        // Every space's walked bytes = live + dead + stubs (the walk is exhaustive).
        for s in d["arena"]["spaces"].as_array().unwrap() {
            let walked = s["walked_bytes"].as_u64().unwrap();
            let sum = s["live"]["bytes"].as_u64().unwrap()
                + s["dead"]["bytes"].as_u64().unwrap()
                + s["late_marked"]["bytes"].as_u64().unwrap()
                + s["stub_live"]["bytes"].as_u64().unwrap()
                + s["stub_dead"]["bytes"].as_u64().unwrap();
            assert_eq!(walked, sum, "space {} walk is exhaustive", s["space"]);
        }
        // Side tables are reported and the process block is populated.
        assert!(!d["side_tables"].as_array().unwrap().is_empty());
        assert!(d["process"]["rss_bytes"].as_u64().unwrap() > 0);
    })
    .join()
    .expect("census test thread must not panic");
}

#[test]
fn census_is_inert_without_a_path() {
    std::thread::spawn(|| {
        let _copying = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        reset_global_roots();
        let _root_reset = ShadowAndGlobalRootResetGuard;
        super::super::census::test_set_census_path(None);
        // Arming without a path is a no-op; the full cycle below must not
        // create any file or touch anything. (There is no path to check —
        // the point is that this runs clean and the armed flag stays down.)
        super::super::census::census_arm("never");
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual));
        // A later real path must not inherit a stale arm from the call above.
        assert!(!super::super::census::test_is_armed());
    })
    .join()
    .expect("census inert test thread must not panic");
}
