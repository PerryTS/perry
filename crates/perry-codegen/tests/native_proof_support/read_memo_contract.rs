//! #12986: selected read sites pair a validated memo leaf with its complete miss.

pub fn paired_memo_reads(ir: &str) -> Option<usize> {
    let mut count = 0;
    for line in ir.lines().filter(|line| {
        line.contains("call double @perry_probe_get_memo(")
            || line.contains("call double @perry_probe_get_value_memo(")
    }) {
        let (_, site) = line.rsplit_once(", ptr @")?;
        if !ir.lines().any(|miss| {
            (miss.contains("call double @js_object_get_field_value_site(")
                || miss.contains("call double @js_object_get_field_site_slow("))
                && miss
                    .rsplit_once(", ptr @")
                    .is_some_and(|(_, other)| other == site)
        }) {
            return None;
        }
        count += 1;
    }
    (count > 0).then_some(count)
}

#[test]
fn memo_pair_gate_rejects_missing_or_wrong_site_misses() {
    let leaf = "%v = call double @perry_probe_get_memo(ptr %d, i64 %o, i64 %k, ptr @site)";
    let miss = "%v = call double @js_object_get_field_value_site(i64 %o, i64 %k, i64 0, ptr @site)";
    assert_eq!(paired_memo_reads(&format!("{leaf}\n{miss}")), Some(1));
    assert_eq!(paired_memo_reads(leaf), None);
    assert_eq!(
        paired_memo_reads(&format!("{leaf}\n{}", miss.replace("@site)", "@other)"))),
        None
    );
    assert_eq!(paired_memo_reads(miss), None);
}
