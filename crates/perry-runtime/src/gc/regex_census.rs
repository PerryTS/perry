//! RegExp side-table serialization for the requested heap census.

use super::census::SideTableRow;

/// Serialize ordinary and RegExp-owned registries through one path. The regex
/// attribution is computed independently of the emitted rows so omission is a
/// visible reconciliation failure rather than a silently smaller total.
pub(super) fn side_table_document_from(mut ordinary: Vec<SideTableRow>) -> serde_json::Value {
    // Replace the legacy RegExp tuples with the rich, reconciled rows.
    ordinary.retain(|(table, _, _)| !table.starts_with("regex."));
    let non_regex_total = ordinary.iter().map(|(_, _, bytes)| *bytes).sum::<usize>();
    // `rows` is extended only under `regex-engine`; without that feature the
    // binding is never mutated, so scope the allow to that configuration rather
    // than dropping `mut` (which breaks the feature-on build).
    #[cfg_attr(not(feature = "regex-engine"), allow(unused_mut))]
    let mut rows = ordinary
        .drain(..)
        .map(|(table, entries, bytes)| {
            serde_json::json!({"table": table, "entries": entries, "bytes": bytes})
        })
        .collect::<Vec<_>>();

    // Perex keeps no Rust-side program tables: a compiled program is a GC
    // allocation owned through its RegExp header, so the ordinary heap census
    // already counts it and there is nothing to attribute separately here.
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
