//! Observational root-pressure report for the native-stack GC experiments.
//!
//! `perry compile --statepoint-report[=text|json]` records how many textual
//! calls see live roots, which ones can be omitted after the GC-effect audit,
//! and how much statepoint/stack-map metadata remains. Codegen never reads the
//! data back, so enabling the report cannot affect emitted IR.
//!
//! `PERRY_STATEPOINT_REPORT` is how the driver carries that flag across to the
//! rayon module workers — the driver sets it, nothing else should. It is not a
//! user-facing knob: accepting it from the environment made it a fifth GC env
//! knob with no CI arm, so that spelling was deleted under CLAUDE.md's GC knob
//! kill policy. `gc-native-roots.yml` exercises the report through the flag.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct FunctionRecord {
    function: String,
    backend: String,
    reserved_logical_slots: u32,
    bound_native_slots: usize,
    textual_calls: u64,
    calls_without_live_roots: u64,
    calls_with_live_roots: u64,
    skipped_non_safepoints: u64,
    statepoints: u64,
    relocations: u64,
    max_live_roots: usize,
    live_roots_histogram: BTreeMap<usize, u64>,
    statepoints_by_callee: BTreeMap<String, u64>,
    skipped_by_callee: BTreeMap<String, u64>,
}

impl FunctionRecord {
    pub(crate) fn new(
        function: &str,
        backend: &str,
        reserved_logical_slots: u32,
        bound_native_slots: usize,
    ) -> Self {
        Self {
            function: function.to_string(),
            backend: backend.to_string(),
            reserved_logical_slots,
            bound_native_slots,
            ..Self::default()
        }
    }

    pub(crate) fn note_call(&mut self, live_roots: usize) {
        self.textual_calls += 1;
        if live_roots == 0 {
            self.calls_without_live_roots += 1;
        } else {
            self.calls_with_live_roots += 1;
        }
    }

    pub(crate) fn note_skipped(&mut self, callee: &str) {
        self.skipped_non_safepoints += 1;
        *self
            .skipped_by_callee
            .entry(callee.to_string())
            .or_default() += 1;
    }

    fn note_emitted_roots(&mut self, live_roots: usize) {
        self.max_live_roots = self.max_live_roots.max(live_roots);
        *self.live_roots_histogram.entry(live_roots).or_default() += 1;
    }

    pub(crate) fn note_statepoint(&mut self, callee: &str, live_roots: usize) {
        self.statepoints += 1;
        self.relocations += live_roots as u64;
        self.note_emitted_roots(live_roots);
        *self
            .statepoints_by_callee
            .entry(callee.to_string())
            .or_default() += 1;
    }
}

pub fn enabled() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_STATEPOINT_REPORT").as_deref(),
            Ok("1") | Ok("text") | Ok("json")
        )
    })
}

static SINK: OnceLock<Mutex<Vec<FunctionRecord>>> = OnceLock::new();

fn sink() -> &'static Mutex<Vec<FunctionRecord>> {
    SINK.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn record(record: FunctionRecord) {
    if enabled() {
        if let Ok(mut records) = sink().lock() {
            records.push(record);
        }
    }
}

pub fn take_records() -> Vec<FunctionRecord> {
    let mut records = match sink().lock() {
        Ok(mut records) => std::mem::take(&mut *records),
        Err(_) => Vec::new(),
    };
    records.sort_by(|a, b| {
        (&a.function, &a.backend, a.reserved_logical_slots).cmp(&(
            &b.function,
            &b.backend,
            b.reserved_logical_slots,
        ))
    });
    records.dedup();
    records
}

#[derive(Default, serde::Serialize)]
struct Totals {
    functions: usize,
    reserved_logical_slots: u64,
    bound_native_slots: u64,
    textual_calls: u64,
    calls_without_live_roots: u64,
    calls_with_live_roots: u64,
    skipped_non_safepoints: u64,
    statepoints: u64,
    relocations: u64,
    max_live_roots: usize,
    live_roots_histogram: BTreeMap<usize, u64>,
    statepoints_by_callee: BTreeMap<String, u64>,
    skipped_by_callee: BTreeMap<String, u64>,
}

fn totals(records: &[FunctionRecord]) -> Totals {
    let mut out = Totals {
        functions: records.len(),
        ..Totals::default()
    };
    for record in records {
        out.reserved_logical_slots += u64::from(record.reserved_logical_slots);
        out.bound_native_slots += record.bound_native_slots as u64;
        out.textual_calls += record.textual_calls;
        out.calls_without_live_roots += record.calls_without_live_roots;
        out.calls_with_live_roots += record.calls_with_live_roots;
        out.skipped_non_safepoints += record.skipped_non_safepoints;
        out.statepoints += record.statepoints;
        out.relocations += record.relocations;
        out.max_live_roots = out.max_live_roots.max(record.max_live_roots);
        for (width, count) in &record.live_roots_histogram {
            *out.live_roots_histogram.entry(*width).or_default() += count;
        }
        for (callee, count) in &record.statepoints_by_callee {
            *out.statepoints_by_callee.entry(callee.clone()).or_default() += count;
        }
        for (callee, count) in &record.skipped_by_callee {
            *out.skipped_by_callee.entry(callee.clone()).or_default() += count;
        }
    }
    out
}

fn render_ranked_map(out: &mut String, heading: &str, values: &BTreeMap<String, u64>) {
    if values.is_empty() {
        return;
    }
    let mut rows: Vec<_> = values.iter().collect();
    rows.sort_by(|(name_a, count_a), (name_b, count_b)| {
        count_b.cmp(count_a).then_with(|| name_a.cmp(name_b))
    });
    let _ = writeln!(out, "{heading}");
    for (name, count) in rows.into_iter().take(25) {
        let _ = writeln!(out, "  {count:>6}  {name}");
    }
    out.push('\n');
}

pub fn render_text(records: &[FunctionRecord]) -> String {
    let totals = totals(records);
    let mut out = String::from(
        "Perry native-stack GC report (--statepoint-report)\n\
         ==================================================\n\n",
    );
    if records.is_empty() {
        out.push_str(
            "No native-stack lowering records were emitted. Set PERRY_RS4GC=1 and\n\
             ensure codegen is not served from cache (PERRY_NO_AUTO_OPTIMIZE=1, or\n\
             clear the object cache) — a cached .o emits no records.\n",
        );
        return out;
    }

    let _ = writeln!(
        out,
        "{} function(s), {} bound native root slots ({} logical slots reserved)",
        totals.functions, totals.bound_native_slots, totals.reserved_logical_slots
    );
    let _ = writeln!(
        out,
        "{} textual calls: {} with live roots, {} without",
        totals.textual_calls, totals.calls_with_live_roots, totals.calls_without_live_roots
    );
    let _ = writeln!(
        out,
        "{} statepoints emitted; {} non-collecting calls skipped",
        totals.statepoints, totals.skipped_non_safepoints
    );
    let _ = writeln!(
        out,
        "{} relocations; maximum {} live roots at one safepoint\n",
        totals.relocations, totals.max_live_roots
    );

    if !totals.live_roots_histogram.is_empty() {
        out.push_str("Live roots per emitted safepoint\n");
        for (width, count) in &totals.live_roots_histogram {
            let _ = writeln!(out, "  {width:>4} root(s): {count:>6} safepoint(s)");
        }
        out.push('\n');
    }
    render_ranked_map(
        &mut out,
        "Most frequent explicit statepoint callees",
        &totals.statepoints_by_callee,
    );
    render_ranked_map(
        &mut out,
        "Calls omitted by the GC-effect audit",
        &totals.skipped_by_callee,
    );
    out
}

#[derive(serde::Serialize)]
struct JsonReport<'a> {
    schema_version: u32,
    totals: Totals,
    functions: &'a [FunctionRecord],
}

pub fn render_json(records: &[FunctionRecord]) -> String {
    serde_json::to_string_pretty(&JsonReport {
        schema_version: 1,
        totals: totals(records),
        functions: records,
    })
    .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_json_expose_root_pressure_and_fallbacks() {
        let mut record = FunctionRecord::new("probe", "statepoint", 3, 2);
        record.note_call(2);
        record.note_statepoint("@may_collect", 2);
        record.note_call(1);
        record.note_skipped("@js_gc_temp_root_get");
        record.note_call(1);

        let text = render_text(std::slice::from_ref(&record));
        assert!(text.contains("2 bound native root slots"));
        assert!(text.contains("1 non-collecting calls skipped"));
        assert!(text.contains("@js_gc_temp_root_get"));

        let json = render_json(&[record]);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["totals"]["relocations"], 2);
    }

    /// Every counter this report prints must have a writer.
    ///
    /// It did not. `plain_stack_maps`, `stack_map_operands`,
    /// `statepoint_fallbacks` and `fallbacks_by_callee` were declared, summed
    /// and rendered, and **no mutator ever wrote them** — `git log -S
    /// note_fallback` finds nothing, so they were never populated, not even
    /// before the plain-map bridge was deleted. The report printed
    /// "0 statepoint parser fallback(s)" as reassurance, a test asserted that
    /// zero, and the comment above that assert said the structural zero "is
    /// the point". A counter that cannot be non-zero is not evidence; it is
    /// CLAUDE.md's fourth failure mode with the subject removed entirely.
    ///
    /// The real fail-closed guarantee is in `gc_map.rs`, which returns `Err`
    /// on an unparseable or uncompactable map, so a fallback fails the BUILD
    /// rather than incrementing a number nobody reads.
    ///
    /// This test pins the invariant that let the dead fields hide: a totals
    /// field that is always zero for a record with real activity is either
    /// unwritten or misrendered.
    #[test]
    fn every_rendered_counter_has_a_writer() {
        let mut record = FunctionRecord::new("f", "rs4gc", 2, 2);
        // Both call shapes: `calls_without_live_roots` only moves for a call
        // with an empty live set, so a fixture of all-live calls would accuse
        // a perfectly live field of having no writer.
        record.note_call(1);
        record.note_call(0);
        record.note_statepoint("@js_alloc", 1);
        record.note_skipped("@js_gc_temp_root_get");

        let json = render_json(&[record]);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let totals = parsed["totals"].as_object().expect("totals is an object");

        for (name, value) in totals {
            // Maps and histograms carry their own emptiness; scalars are the
            // ones that silently read as "checked, and fine".
            let Some(n) = value.as_u64() else { continue };
            assert_ne!(
                n, 0,
                "totals.{name} is zero for a record with a call, a statepoint \
                 and a skip — it has no writer, or nothing reaches it. Give it \
                 one or delete the field; do not print a number that cannot move."
            );
        }
    }
}
