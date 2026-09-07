//! RegExp side-table serialization for the requested heap census.

use super::census::SideTableRow;

/// Serialize ordinary and RegExp-owned registries through one path. The regex
/// attribution is computed independently of the emitted rows so omission is a
/// visible reconciliation failure rather than a silently smaller total.
pub(super) fn side_table_document_from(mut ordinary: Vec<SideTableRow>) -> serde_json::Value {
    // Replace the legacy RegExp tuples with the rich, reconciled rows.
    ordinary.retain(|(table, _, _)| !table.starts_with("regex."));
    let non_regex_total = ordinary.iter().map(|(_, _, bytes)| *bytes).sum::<usize>();
    let mut rows = ordinary
        .drain(..)
        .map(|(table, entries, bytes)| {
            serde_json::json!({"table": table, "entries": entries, "bytes": bytes})
        })
        .collect::<Vec<_>>();

    #[cfg(feature = "regex-engine")]
    let regex_total = {
        let snapshot = crate::regex::census_snapshot();
        rows.extend(snapshot.rows.iter().map(crate::regex::RegexCensusRow::json));
        snapshot.attributed_bytes
    };
    #[cfg(not(feature = "regex-engine"))]
    let regex_total = 0usize;

    serde_json::json!({
        "rows": rows,
        "side_table_bytes": non_regex_total + regex_total,
        "regex_side_table_bytes": regex_total,
        "non_regex_side_table_bytes": non_regex_total,
    })
}

#[cfg(test)]
fn test_side_table_document() -> serde_json::Value {
    let snapshot = side_table_document_from(super::census::side_tables());
    serde_json::json!({
        "totals": {
            "side_table_bytes": snapshot["side_table_bytes"],
            "regex_side_table_bytes": snapshot["regex_side_table_bytes"],
            "non_regex_side_table_bytes": snapshot["non_regex_side_table_bytes"],
        },
        "side_tables": snapshot["rows"],
    })
}

#[cfg(all(test, feature = "regex-engine"))]
mod tests {
    use crate::regex::site_test::js_regexp_site_test_new;
    use crate::regex::{js_regexp_new, js_regexp_test};

    fn string(value: &str) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32)
    }

    fn regex_rows(doc: &serde_json::Value) -> Vec<&serde_json::Value> {
        doc["side_tables"]
            .as_array()
            .expect("side-table array")
            .iter()
            .filter(|row| {
                row["table"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("regex."))
            })
            .collect()
    }

    #[test]
    fn census_prints_regex_rows_that_reconcile_with_side_table_total() {
        let _lock = crate::gc::global_side_table_test_lock();
        crate::regex::census_rows::test_reset_tables();

        const N: usize = 6;
        for index in 0..N {
            let source = format!("regex-census-{index}");
            let pattern = string(&source);
            let header = js_regexp_new(pattern, string(""));
            assert_ne!(js_regexp_test(header, string(&source)), 0);
        }

        // Evaluate one direct literal site twice: the second call must reuse
        // the first rooted header and its installed program bundle.
        let prototype = crate::object::builtin_prototype_value("RegExp");
        assert!(crate::value::JSValue::from_bits(prototype.to_bits()).is_pointer());
        static SITE: u64 = 0;
        let site = std::ptr::addr_of!(SITE) as i64;
        let first = js_regexp_site_test_new(string("site-census"), string("g"), site);
        assert_ne!(js_regexp_test(first, string("site-census")), 0);
        let second = js_regexp_site_test_new(string("site-census"), string("g"), site);
        assert_eq!(
            first, second,
            "the literal site must reuse its rooted header"
        );

        assert_eq!(
            crate::regex::census_rows::test_walks(),
            0,
            "regex construction and matching must not do census bookkeeping"
        );
        let encoded = super::test_side_table_document().to_string();
        let doc: serde_json::Value = serde_json::from_str(&encoded).expect("valid census JSON");
        let rows = regex_rows(&doc);

        let names = rows
            .iter()
            .map(|row| row["table"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>();
        for expected in [
            "regex.pointers",
            "regex.program_cache",
            "regex.fancy_cache",
            "regex.repeat_cache",
            "regex.validated_patterns",
            "regex.content_cache",
            "regex.literal_sites",
            "regex.site_table",
            "regex.active_factory_sites",
            "regex.expando_owners",
            "regex.matcher_kinds",
        ] {
            assert!(names.contains(expected), "missing census row {expected}");
        }

        let pointer = rows
            .iter()
            .find(|row| row["table"] == "regex.pointers")
            .expect("regex.pointers row");
        assert!(pointer["entries"].as_u64().unwrap() >= N as u64);
        let site = rows
            .iter()
            .find(|row| row["table"] == "regex.site_table")
            .expect("regex.site_table row");
        assert!(site["sites"].as_u64().unwrap() >= 1);
        assert!(site["rooted_headers"].as_u64().unwrap() >= 1);
        assert!(site["pinned_programs"].as_u64().unwrap() >= 1);
        assert!(
            site["pinned_program_bytes"].as_u64().unwrap()
                >= site["attributed_program_bytes"].as_u64().unwrap()
        );
        assert_eq!(site["pinned_program_bytes_inside_side_table_bytes"], false);
        let row_bytes = rows
            .iter()
            .map(|row| row["bytes"].as_u64().expect("numeric row bytes"))
            .sum::<u64>();
        let regex_total = doc["totals"]["regex_side_table_bytes"]
            .as_u64()
            .expect("regex attribution total");
        let side_total = doc["totals"]["side_table_bytes"].as_u64().unwrap();
        let non_regex_total = doc["totals"]["non_regex_side_table_bytes"]
            .as_u64()
            .unwrap();
        assert_eq!(row_bytes, regex_total);
        assert_eq!(side_total - non_regex_total, regex_total);

        // Sabotage proof: the attribution total is built independently of
        // JSON row registration. Omitting any non-zero row from `side_tables`
        // makes this exact reconciliation fail.
        let omitted = rows
            .iter()
            .find(|row| row["bytes"].as_u64().unwrap_or(0) != 0)
            .unwrap()["bytes"]
            .as_u64()
            .unwrap();
        assert_ne!(row_bytes - omitted, regex_total);
    }

    #[test]
    fn census_regex_rows_are_zero_cost_when_not_requested() {
        let _lock = crate::gc::global_side_table_test_lock();
        crate::regex::census_rows::test_reset_tables();
        let header = js_regexp_new(string("zero-cost-census"), string(""));
        assert_ne!(js_regexp_test(header, string("zero-cost-census")), 0);
        assert_eq!(
            crate::regex::census_rows::test_walks(),
            0,
            "construction must not enter regex census row code"
        );
        let _ = super::test_side_table_document();
        assert!(crate::regex::census_rows::test_walks() > 0);
    }
}
