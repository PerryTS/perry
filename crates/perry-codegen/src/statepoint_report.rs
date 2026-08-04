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
}

/// What the compact-map rewrite actually found in the emitted assembly.
///
/// This is the ONLY honest post-RS4GC source for these numbers. Perry no
/// longer chooses which calls become safepoints — `RewriteStatepointsForGC`
/// does, inside LLVM — so counting at IR-emission time cannot work, and the
/// counters that tried were silently zero (see `note_gc_map`'s callers and
/// the regression note on `render_text`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct GcMapTotals {
    /// Functions carrying at least one safepoint with a live root.
    pub functions: u64,
    /// Safepoints, i.e. stack-map records.
    pub records: u64,
    /// (safepoint, root) pairs — the relocations LLVM emitted.
    pub roots: u64,
    /// Modules whose stack map was compacted. Zero here with non-zero
    /// function records is the "instrument not wired" case.
    pub modules: u64,
}

static GC_MAP: Mutex<GcMapTotals> = Mutex::new(GcMapTotals {
    functions: 0,
    records: 0,
    roots: 0,
    modules: 0,
});

/// Record one module's compact-map result. Called from `gc_map::compact_and_assemble`.
pub(crate) fn note_gc_map(functions: usize, records: usize, roots: usize) {
    if !enabled() {
        return;
    }
    if let Ok(mut totals) = GC_MAP.lock() {
        totals.functions += functions as u64;
        totals.records += records as u64;
        totals.roots += roots as u64;
        totals.modules += 1;
    }
}

fn take_gc_map() -> GcMapTotals {
    match GC_MAP.lock() {
        Ok(mut totals) => std::mem::take(&mut *totals),
        Err(_) => GcMapTotals::default(),
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
    render_text_with(records, take_gc_map())
}

fn render_text_with(records: &[FunctionRecord], gc_map: GcMapTotals) -> String {
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
    // Everything above is counted at IR-emission time. Everything below comes
    // from the compact-map rewrite, which parses the assembly LLVM actually
    // emitted — the only honest source now that RS4GC, not Perry, decides
    // which calls become safepoints.
    if gc_map.modules == 0 {
        out.push_str(
            "\nSafepoint counts UNAVAILABLE: the compact-map rewrite never reported.\n\
             The report was rendered anyway rather than printing zeros, because a\n\
             confident `0 statepoints emitted` is indistinguishable from a real\n\
             zero and that is exactly how these counts silently died once before\n\
             (#7348 removed their only writers with the bridge, and nothing\n\
             noticed). Compile with PERRY_RS4GC=1 on a target whose map is\n\
             rewritten, and without a cached .o.\n",
        );
        return out;
    }

    let _ = writeln!(
        out,
        "{} safepoints across {} function(s) in {} module(s)",
        gc_map.records, gc_map.functions, gc_map.modules
    );
    let mean = if gc_map.records == 0 {
        0.0
    } else {
        gc_map.roots as f64 / gc_map.records as f64
    };
    let _ = writeln!(
        out,
        "{} live roots recorded, {mean:.2} per safepoint\n",
        gc_map.roots
    );
    out
}

#[derive(serde::Serialize)]
struct JsonReport<'a> {
    schema_version: u32,
    totals: Totals,
    /// Safepoint counts from the compact-map rewrite. `modules: 0` means the
    /// rewrite never reported, so `records`/`roots` are absent rather than
    /// zero — a consumer must be able to tell "no safepoints" from "not
    /// measured", which is the distinction that went missing in #7348.
    gc_map: GcMapTotals,
    functions: &'a [FunctionRecord],
}

pub fn render_json(records: &[FunctionRecord]) -> String {
    render_json_with(records, take_gc_map())
}

fn render_json_with(records: &[FunctionRecord], gc_map: GcMapTotals) -> String {
    serde_json::to_string_pretty(&JsonReport {
        schema_version: 2,
        totals: totals(records),
        gc_map,
        functions: records,
    })
    .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_record() -> FunctionRecord {
        let mut record = FunctionRecord::new("probe", "rs4gc", 3, 2);
        // Both call shapes: `calls_without_live_roots` only moves for a call
        // with an empty live set, so a fixture of all-live calls would accuse
        // a perfectly live field of having no writer.
        record.note_call(2);
        record.note_call(0);
        record
    }

    #[test]
    fn text_and_json_expose_root_pressure() {
        let record = probe_record();
        let map = GcMapTotals {
            functions: 1,
            records: 4,
            roots: 9,
            modules: 1,
        };

        let text = render_text_with(std::slice::from_ref(&record), map);
        assert!(text.contains("2 bound native root slots"));
        assert!(text.contains("4 safepoints across 1 function(s) in 1 module(s)"));
        // 9 / 4 — the mean must come out of the real counts, not a placeholder.
        assert!(
            text.contains("9 live roots recorded, 2.25 per safepoint"),
            "{text}"
        );

        let json = render_json_with(&[record], map);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["schema_version"], 2);
        assert_eq!(parsed["gc_map"]["records"], 4);
        assert_eq!(parsed["gc_map"]["roots"], 9);
    }

    /// Every counter this report prints must have a writer.
    ///
    /// Two rounds of this have now been needed, and the second is the reason
    /// the first was not enough.
    ///
    /// **Round one.** `plain_stack_maps`, `stack_map_operands`,
    /// `statepoint_fallbacks` and `fallbacks_by_callee` were declared, summed
    /// and rendered with no mutator writing them, ever. The report printed
    /// "0 statepoint parser fallback(s)" as reassurance and a test asserted
    /// that zero, above a comment saying the structural zero "is the point".
    ///
    /// **Round two, which this test originally missed.** `statepoints`,
    /// `relocations`, `max_live_roots` and the skip counters *did* have
    /// mutators — and #7348 deleted their only production callers along with
    /// the explicit bridge. The methods survived, so the invariant below still
    /// passed: the test called them itself. Meanwhile every real compile
    /// printed `0 statepoints emitted` while its binary carried thousands.
    ///
    /// The lesson is that "has a writer" is weaker than "is written". The
    /// structural fix is not a bigger assertion here — it is that the numbers
    /// now come from exactly one place (`note_gc_map`, fed by the compact-map
    /// rewrite) and that their absence renders as an explicit UNAVAILABLE
    /// notice rather than a zero. See `absent_map_counts_say_so_instead_of_zero`.
    #[test]
    fn every_rendered_counter_has_a_writer() {
        let json = render_json_with(
            &[probe_record()],
            GcMapTotals {
                functions: 1,
                records: 1,
                roots: 1,
                modules: 1,
            },
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        for section in ["totals", "gc_map"] {
            let obj = parsed[section]
                .as_object()
                .unwrap_or_else(|| panic!("{section} is an object"));
            for (name, value) in obj {
                let Some(n) = value.as_u64() else { continue };
                assert_ne!(
                    n, 0,
                    "{section}.{name} is zero for a record with real activity — \
                     it has no writer, or nothing reaches it. Give it one or \
                     delete the field; do not print a number that cannot move."
                );
            }
        }
    }

    /// A missing measurement must not render as a measured zero.
    ///
    /// This is the guard that would have caught #7348 the day it landed.
    /// `--statepoint-report` kept printing `0 statepoints emitted; 0
    /// non-collecting calls skipped` after its writers were deleted, and
    /// nothing distinguished that from a program with genuinely no safepoints.
    #[test]
    fn absent_map_counts_say_so_instead_of_zero() {
        let text = render_text_with(&[probe_record()], GcMapTotals::default());
        assert!(
            text.contains("Safepoint counts UNAVAILABLE"),
            "an unreported map must say so:\n{text}"
        );
        assert!(
            !text.contains("0 safepoints"),
            "a confident zero is the bug this test exists for:\n{text}"
        );

        let json = render_json_with(&[probe_record()], GcMapTotals::default());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["gc_map"]["modules"], 0,
            "a JSON consumer must be able to tell not-measured from zero"
        );
    }
}
