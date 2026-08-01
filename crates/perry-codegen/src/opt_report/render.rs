//! Renderers for `--opt-report` (#6952): human-readable text and a stable
//! JSON schema.
//!
//! The text renderer's first job is the one thing #7034 asked for: state
//! plainly, in one command, how many candidates each representation promoted
//! — *including when the answer is zero* — and name the rule that denied each
//! one. The wins are reported alongside the misses on purpose: a report that
//! only nags is less useful, and less trusted, than one that shows the ratio.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{Analysis, Entry, Outcome, Tier};

/// Bump when a field is removed or its meaning changes. Additive fields do
/// not require a bump — consumers must ignore unknown keys.
///
/// v2 (#7106 follow-up) splits a promotion's *selection* from its
/// *consumption*. `selected` keeps its old meaning (an analysis proved the
/// value) but explicitly stops implying that codegen emitted anything for it;
/// `consumed` / `unconsumed` carry that. A consumer that keyed performance
/// claims off `selected` under v1 was reading a number that does not mean what
/// it looks like, so this is a meaning change and takes a bump.
pub const SCHEMA_VERSION: u32 = 2;

/// How many denials to show per tier before collapsing the tail. The cold
/// tail is real information but it is not the actionable part.
const MAX_ROWS_PER_TIER: usize = 25;

#[derive(Debug, Clone, Default, serde::Serialize)]
struct AnalysisTally {
    selected: usize,
    denied: usize,
    /// Selected values codegen actually applied the representation to.
    consumed: usize,
    /// Selected values codegen reached and dropped the proof for, with a named
    /// mechanism. `selected - consumed - unconsumed` is the residue: values
    /// with no access site at all, which no mechanism can be blamed for.
    unconsumed: usize,
}

impl AnalysisTally {
    fn candidates(&self) -> usize {
        self.selected + self.denied
    }
}

fn tally(entries: &[Entry]) -> BTreeMap<Analysis, AnalysisTally> {
    let mut out: BTreeMap<Analysis, AnalysisTally> = BTreeMap::new();
    for e in entries {
        let slot = out.entry(e.analysis).or_default();
        match e.outcome {
            Outcome::Selected => slot.selected += 1,
            Outcome::Denied => slot.denied += 1,
            Outcome::Consumed => slot.consumed += 1,
            Outcome::Unconsumed => slot.unconsumed += 1,
        }
    }
    out
}

/// One-line hotness annotation. Loop depth and per-element invocation are
/// reported as *separate* facts — #7034 §8 measured that 208 of 247 guard
/// sites in the motivating workload sit in callback bodies with zero loop
/// depth, so collapsing them into one "hotness" number would rank them wrong.
fn hotness(e: &Entry) -> String {
    match (&e.invoked_per_element, e.loop_depth) {
        (Some(builtin), 0) => format!("per-element callback of {builtin}"),
        (Some(builtin), d) => format!("per-element callback of {builtin}, loop depth {d}"),
        (None, 0) => String::from("loop depth 0"),
        (None, d) => format!("loop depth {d}"),
    }
}

/// Human-readable report.
pub fn render_text(entries: &[Entry]) -> String {
    let mut out = String::new();
    let tallies = tally(entries);

    out.push_str("Perry optimization report (--opt-report)\n");
    out.push_str("========================================\n\n");

    if entries.is_empty() {
        out.push_str(
            "No representation decisions were recorded.\n\n\
             This means codegen did not run for any module — most often an object-cache\n\
             hit. `--opt-report` disables the object and build caches for its own run, so\n\
             if you are seeing this, the build produced no native modules at all.\n",
        );
        return out;
    }

    // ── Summary: wins and misses side by side ──────────────────────────────
    let total_selected: usize = tallies.values().map(|t| t.selected).sum();
    let total_denied: usize = tallies.values().map(|t| t.denied).sum();
    out.push_str("Representation summary\n");
    out.push_str("----------------------\n");
    for (analysis, t) in &tallies {
        let pct = if t.candidates() == 0 {
            0
        } else {
            t.selected * 100 / t.candidates()
        };
        let _ = writeln!(
            out,
            "  {:<16} {:>4} selected / {:>4} denied   ({pct}% of {} candidates)",
            analysis.target_rep(),
            t.selected,
            t.denied,
            t.candidates(),
        );
        // Printed whenever the analysis is INSTRUMENTED, not whenever the
        // numbers happen to be nonzero. An analysis whose promotions were all
        // wasted has `consumed == 0` and possibly no mechanism record either;
        // staying silent there would render the worst case identically to an
        // uninstrumented analysis, which is the confusion this column exists to
        // remove.
        if analysis.records_consumption() {
            let _ = writeln!(
                out,
                "  {:<16} {:>4} of those selections were CONSUMED by codegen{}",
                "",
                t.consumed,
                if t.unconsumed > 0 {
                    format!(", {} dropped unused", t.unconsumed)
                } else {
                    String::new()
                },
            );
        }
    }
    let _ = writeln!(
        out,
        "  {:<16} {total_selected} values unboxed, {total_denied} left Boxed",
        "TOTAL",
    );
    let masked = super::masked_by_dedup();
    if masked > 0 {
        let _ = writeln!(
            out,
            "  {:<16} {masked} duplicate row(s) collapsed by de-duplication",
            "",
        );
    }
    out.push('\n');

    // ── Unbound allocation sites, by position ──────────────────────────────
    // #7170 R0. These rows are the majority of the `Ptr<Shape>` denial
    // population on real code, and their positions are what a scheduler reads
    // to decide which escape to open next. Printed as an explicit table so
    // that reading is not a `jq` reduction over `entries[]` whose bucket
    // definitions live nowhere.
    let alloc = alloc_buckets(entries);
    if !alloc.is_empty() {
        let total: usize = alloc.iter().map(|b| b.denied).sum();
        let _ = writeln!(out, "Unbound allocation sites by position ({total})");
        out.push_str("-------------------------------------\n");
        for b in &alloc {
            let _ = writeln!(
                out,
                "  {:<34} {:>5}   {} / {}",
                b.context, b.denied, b.analysis, b.rule,
            );
        }
        out.push('\n');
    }

    // ── Headline: call out any representation that promoted nothing ────────
    // This is the #7034 §0 finding, stated without having to read IR.
    let mut zeros: Vec<(&Analysis, &AnalysisTally)> = tallies
        .iter()
        .filter(|(_, t)| t.selected == 0 && t.denied > 0)
        .collect();
    zeros.sort_by_key(|(a, _)| **a);
    for (analysis, t) in zeros {
        let _ = writeln!(
            out,
            "*** {} promoted 0 of {} candidates in this build. ***",
            analysis.target_rep(),
            t.candidates(),
        );
        let _ = writeln!(
            out,
            "    Every candidate was denied; the rules are in {}.\n",
            analysis.rule_source(),
        );
    }

    // ── Wasted promotions ──────────────────────────────────────────────────
    // A selection that codegen dropped is worse than a denial: a denial names a
    // rule and shows up as a zero, while this shows up as a WIN. #7107 found by
    // reading IR that `batch.ts` reports two `Ptr<Shape>` promotions and applies
    // one. Nothing in the report said so.
    let wasted: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.outcome == Outcome::Unconsumed)
        .collect();
    if !wasted.is_empty() {
        let _ = writeln!(
            out,
            "Selected but NOT consumed ({}) — counted as wins, emitted nothing",
            wasted.len(),
        );
        out.push_str("--------------------------------------------------------------\n");
        for e in wasted.iter().take(MAX_ROWS_PER_TIER) {
            let _ = writeln!(
                out,
                "  {} :: {} `{}` -> {} (in {} {})",
                e.module,
                e.position.as_str(),
                e.name,
                e.rep,
                e.region.as_str(),
                e.function,
            );
            if let Some(rule) = &e.rule {
                let _ = writeln!(out, "      dropped by {rule}");
            }
            if let Some(reason) = &e.reason {
                let _ = writeln!(out, "      {reason}");
            }
            if let Some(issue) = &e.issue {
                let _ = writeln!(out, "      tracking: {issue}");
            }
        }
        if wasted.len() > MAX_ROWS_PER_TIER {
            let _ = writeln!(out, "  ... and {} more", wasted.len() - MAX_ROWS_PER_TIER);
        }
        out.push('\n');
    }

    // ── Denials, ranked, grouped by actionability tier ─────────────────────
    let denials: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.outcome == Outcome::Denied)
        .collect();
    if denials.is_empty() {
        out.push_str("No denied values — every proven-typeable value got an unboxed\n");
        out.push_str("representation in this build.\n\n");
    } else {
        out.push_str("Denied values, hottest first\n");
        out.push_str("----------------------------\n");
        out.push_str(
            "Hotness is a static proxy, not a profile. `per-element callback` means the\n\
             enclosing region is an iterating builtin's callback: it has no loop of its\n\
             own but runs once per element, so loop depth alone would rank it last.\n\n",
        );
    }

    for tier in Tier::ALL {
        let rows: Vec<&&Entry> = denials
            .iter()
            .filter(|e| e.tier == Some(tier))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(out, "{} ({} value(s))", tier.heading(), rows.len());
        for e in rows.iter().take(MAX_ROWS_PER_TIER) {
            let _ = writeln!(
                out,
                "  {} :: {} `{}` [{}]",
                e.module,
                e.position.as_str(),
                e.name,
                hotness(e),
            );
            let _ = writeln!(
                out,
                "      in {} {} -> {}",
                e.region.as_str(),
                e.function,
                e.rep,
            );
            if let Some(rule) = &e.rule {
                let _ = writeln!(out, "      {} {rule}", e.analysis.as_str());
            }
            if let Some(reason) = &e.reason {
                let _ = writeln!(out, "      {reason}");
            }
            if let Some(detail) = &e.detail {
                let _ = writeln!(out, "      {detail}");
            }
            if let Some(ordinal) = e.alloc_ordinal {
                let _ = writeln!(out, "      allocation site #{ordinal} in this region");
            }
            if let Some(offset) = e.byte_offset {
                let _ = writeln!(out, "      source byte offset {offset}");
            }
            if let Some(issue) = &e.issue {
                let _ = writeln!(out, "      tracking: {issue}");
            }
        }
        if rows.len() > MAX_ROWS_PER_TIER {
            let _ = writeln!(
                out,
                "  ... and {} more (use --opt-report=json for the full list)",
                rows.len() - MAX_ROWS_PER_TIER,
            );
        }
        out.push('\n');
    }

    // ── Wins, compactly ────────────────────────────────────────────────────
    let wins: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.outcome == Outcome::Selected)
        .collect();
    if !wins.is_empty() {
        let _ = writeln!(out, "Unboxed values ({})", wins.len());
        out.push_str("-----------------\n");
        for e in wins.iter().take(MAX_ROWS_PER_TIER) {
            let _ = writeln!(
                out,
                "  {} :: {} `{}` -> {} (in {} {})",
                e.module,
                e.position.as_str(),
                e.name,
                e.rep,
                e.region.as_str(),
                e.function,
            );
        }
        if wins.len() > MAX_ROWS_PER_TIER {
            let _ = writeln!(out, "  ... and {} more", wins.len() - MAX_ROWS_PER_TIER);
        }
        out.push('\n');
    }

    out.push_str(
        "Values are reported by function and binding name: HIR keeps names through\n\
         lowering but not source spans, so there is no file:line yet.\n",
    );
    out
}

#[derive(Debug, serde::Serialize)]
struct JsonAnalysis<'a> {
    analysis: &'a str,
    target_rep: &'a str,
    rule_source: &'a str,
    selected: usize,
    denied: usize,
    consumed: usize,
    unconsumed: usize,
    /// Whether `consumed` means "nothing applied" or "nobody measured". The
    /// census cross-checks its own table against this, so the two cannot drift.
    records_consumption: bool,
}

/// One `(analysis, rule)` denial bucket.
///
/// #7170 R0: the bucket counts are computed **here**, by the compiler that
/// owns the rule vocabulary, rather than reconstructed downstream with `jq`
/// over `entries[]`. Every scheduling decision in the #7152/#7170 chain was
/// taken off a hand-rolled reduction of those rows, and two of the three
/// defects R0 fixes were invisible in that reduction by construction: it could
/// not see that one label meant two mechanisms, nor that a bucket contained
/// already-served sites.
#[derive(Debug, serde::Serialize)]
struct JsonRuleBucket<'a> {
    analysis: &'a str,
    rule: &'a str,
    tier: Option<Tier>,
    denied: usize,
}

/// One `(allocation position, rule)` bucket, over `alloc-site` denials.
///
/// Separate from [`JsonRuleBucket`] because the position is the discriminant
/// that #7170 §5.1 found merged: `constructor argument` and `object literal
/// property value` share a rule and are different opportunities.
#[derive(Debug, serde::Serialize)]
struct JsonAllocBucket<'a> {
    analysis: &'a str,
    context: &'a str,
    rule: &'a str,
    denied: usize,
}

#[derive(Debug, serde::Serialize)]
struct JsonSummary<'a> {
    selected: usize,
    denied: usize,
    consumed: usize,
    unconsumed: usize,
    /// Entries the drain collapsed. See
    /// [`crate::opt_report::masked_by_dedup`] — a nonzero value here is the
    /// count of rows a consumer is NOT seeing.
    masked_by_dedup: usize,
    by_analysis: Vec<JsonAnalysis<'a>>,
    by_rule: Vec<JsonRuleBucket<'a>>,
    by_alloc_context: Vec<JsonAllocBucket<'a>>,
}

fn rule_buckets(entries: &[Entry]) -> Vec<JsonRuleBucket<'_>> {
    let mut out: BTreeMap<(&str, &str), (Option<Tier>, usize)> = BTreeMap::new();
    for e in entries.iter().filter(|e| e.outcome == Outcome::Denied) {
        let rule = e.rule.as_deref().unwrap_or("<unnamed>");
        let slot = out
            .entry((e.analysis.as_str(), rule))
            .or_insert((e.tier, 0));
        slot.1 += 1;
    }
    out.into_iter()
        .map(|((analysis, rule), (tier, denied))| JsonRuleBucket {
            analysis,
            rule,
            tier,
            denied,
        })
        .collect()
}

fn alloc_buckets(entries: &[Entry]) -> Vec<JsonAllocBucket<'_>> {
    let mut out: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();
    for e in entries.iter().filter(|e| e.outcome == Outcome::Denied) {
        let Some(context) = e.alloc_context.as_deref() else {
            continue;
        };
        let rule = e.rule.as_deref().unwrap_or("<unnamed>");
        *out.entry((e.analysis.as_str(), context, rule)).or_insert(0) += 1;
    }
    out.into_iter()
        .map(|((analysis, context, rule), denied)| JsonAllocBucket {
            analysis,
            context,
            rule,
            denied,
        })
        .collect()
}

#[derive(Debug, serde::Serialize)]
struct JsonReport<'a> {
    schema_version: u32,
    summary: JsonSummary<'a>,
    entries: &'a [Entry],
}

/// Machine-readable report. The schema is stable enough for CI to diff two
/// builds and catch a representation regression (a `selected` count silently
/// going to zero — exactly what #7034 found by hand).
pub fn render_json(entries: &[Entry]) -> String {
    let tallies = tally(entries);
    let report = JsonReport {
        schema_version: SCHEMA_VERSION,
        summary: JsonSummary {
            selected: tallies.values().map(|t| t.selected).sum(),
            denied: tallies.values().map(|t| t.denied).sum(),
            consumed: tallies.values().map(|t| t.consumed).sum(),
            unconsumed: tallies.values().map(|t| t.unconsumed).sum(),
            masked_by_dedup: super::masked_by_dedup(),
            // Enumerate `Analysis::ALL`, not the analyses that happen to have
            // entries: an analysis that recorded nothing must appear with an
            // explicit `0`. A consumer cannot tell an absent key from a zero
            // one, and "zero promotions" is precisely the fact this report
            // exists to make visible (#7034 §0; census #7106).
            by_analysis: Analysis::ALL
                .iter()
                .map(|a| {
                    let t = tallies.get(a).cloned().unwrap_or_default();
                    JsonAnalysis {
                        analysis: a.as_str(),
                        target_rep: a.target_rep(),
                        rule_source: a.rule_source(),
                        selected: t.selected,
                        denied: t.denied,
                        consumed: t.consumed,
                        unconsumed: t.unconsumed,
                        records_consumption: a.records_consumption(),
                    }
                })
                .collect(),
            by_rule: rule_buckets(entries),
            by_alloc_context: alloc_buckets(entries),
        },
        entries,
    };
    serde_json::to_string_pretty(&report).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opt_report::{Position, RegionKind};

    fn denied(analysis: Analysis, name: &str, rule: &str) -> Entry {
        Entry {
            module: "batch.ts".into(),
            function: "buildRows".into(),
            region: RegionKind::Function,
            position: Position::Local,
            name: name.into(),
            local_id: Some(7),
            analysis,
            outcome: Outcome::Denied,
            rep: "Boxed".into(),
            rule: Some(rule.into()),
            reason: Some("used as a call argument".into()),
            tier: Some(Tier::Fixable),
            issue: None,
            loop_depth: 1,
            invoked_per_element: None,
            detail: None,
            byte_offset: None,
            site: None,
            alloc_context: None,
            alloc_ordinal: None,
        }
    }

    fn selected(analysis: Analysis, name: &str, rep: &str) -> Entry {
        Entry {
            module: "batch.ts".into(),
            function: "containedTotals".into(),
            region: RegionKind::Function,
            position: Position::Local,
            name: name.into(),
            local_id: Some(9),
            analysis,
            outcome: Outcome::Selected,
            rep: rep.into(),
            rule: None,
            reason: None,
            tier: None,
            issue: None,
            loop_depth: 0,
            invoked_per_element: None,
            detail: None,
            byte_offset: None,
            site: None,
            alloc_context: None,
            alloc_ordinal: None,
        }
    }

    /// The #7034 §0 acceptance case: when a representation promotes nothing,
    /// the text report must SAY so, not leave the reader to infer it from an
    /// absent section.
    #[test]
    fn zero_promotion_is_stated_explicitly() {
        let entries = vec![
            denied(Analysis::PtrShape, "row", "rule 2 (containment)"),
            denied(Analysis::PtrShape, "s", "rule 2 (containment)"),
        ];
        let text = render_text(&entries);
        assert!(
            text.contains("Ptr<Shape> promoted 0 of 2 candidates"),
            "report must state the zero-promotion headline; got:\n{text}"
        );
        assert!(
            text.contains("collectors/ptr_shape.rs"),
            "report must cite the rule source; got:\n{text}"
        );
        assert!(
            text.contains("rule 2 (containment)"),
            "report must name the denying rule; got:\n{text}"
        );
    }

    /// The headline must NOT fire when the representation did promote
    /// something — otherwise it is noise and gets ignored.
    #[test]
    fn zero_promotion_headline_is_absent_when_something_promoted() {
        let entries = vec![
            denied(Analysis::PtrShape, "row", "rule 2 (containment)"),
            selected(Analysis::PtrShape, "acc", "Ptr<Shape>"),
        ];
        let text = render_text(&entries);
        assert!(
            !text.contains("promoted 0 of"),
            "headline must not fire when a candidate was promoted; got:\n{text}"
        );
        assert!(
            text.contains("1 selected /    1 denied"),
            "summary must show the ratio; got:\n{text}"
        );
    }

    /// Wins are reported, not just misses.
    #[test]
    fn wins_are_reported() {
        let entries = vec![selected(Analysis::CanonicalSlot, "i", "I32")];
        let text = render_text(&entries);
        assert!(text.contains("Unboxed values (1)"), "got:\n{text}");
        assert!(text.contains("`i` -> I32"), "got:\n{text}");
        assert!(
            text.contains("1 values unboxed, 0 left Boxed"),
            "got:\n{text}"
        );
    }

    #[test]
    fn empty_report_explains_itself() {
        let text = render_text(&[]);
        assert!(text.contains("No representation decisions were recorded"));
    }

    #[test]
    fn json_carries_schema_version_and_tallies() {
        let entries = vec![
            denied(Analysis::PtrShape, "row", "rule 2 (containment)"),
            selected(Analysis::CanonicalSlot, "i", "I32"),
        ];
        let json: serde_json::Value = serde_json::from_str(&render_json(&entries)).unwrap();
        assert_eq!(json["schema_version"], SCHEMA_VERSION);
        assert_eq!(json["summary"]["selected"], 1);
        assert_eq!(json["summary"]["denied"], 1);
        assert_eq!(json["entries"].as_array().unwrap().len(), 2);
        let ptr_shape = json["summary"]["by_analysis"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["analysis"] == "ptr-shape")
            .expect("ptr-shape tally present");
        assert_eq!(ptr_shape["selected"], 0);
        assert_eq!(ptr_shape["denied"], 1);
        assert_eq!(ptr_shape["target_rep"], "Ptr<Shape>");
    }

    /// #7106: a census consumer must be able to read "this representation
    /// promoted nothing" straight off the JSON. An analysis with no entries at
    /// all therefore has to render as an explicit zero row, because an absent
    /// key and a zero key are indistinguishable downstream — and "absent" is
    /// exactly what a dead instrument produces.
    #[test]
    fn json_lists_every_analysis_even_with_no_entries() {
        let entries = vec![selected(Analysis::CanonicalSlot, "i", "I32")];
        let json: serde_json::Value = serde_json::from_str(&render_json(&entries)).unwrap();
        let rows = json["summary"]["by_analysis"].as_array().unwrap();
        assert_eq!(
            rows.len(),
            Analysis::ALL.len(),
            "every analysis must have a row; got:\n{rows:#?}"
        );
        for analysis in Analysis::ALL {
            let row = rows
                .iter()
                .find(|r| r["analysis"] == analysis.as_str())
                .unwrap_or_else(|| panic!("{} row missing", analysis.as_str()));
            assert_eq!(row["target_rep"], analysis.target_rep());
        }
        let numarray = rows
            .iter()
            .find(|r| r["analysis"] == "ptr-numarray")
            .unwrap();
        assert_eq!(numarray["selected"], 0);
        assert_eq!(numarray["denied"], 0);
    }

    fn with_outcome(analysis: Analysis, name: &str, outcome: Outcome, rule: Option<&str>) -> Entry {
        let mut e = selected(analysis, name, "Ptr<Shape>");
        e.outcome = outcome;
        if outcome == Outcome::Consumed {
            e.site = Some("ptr_shape_set".to_string());
        }
        e.rule = rule.map(str::to_string);
        e.reason = rule.map(|_| "the context gate dropped it".to_string());
        e.issue = rule.map(|_| "#7109".to_string());
        e
    }

    /// The headline case. A selected-and-dropped promotion must be visible as
    /// such: before this, `batch.ts` reported two `Ptr<Shape>` wins, applied
    /// one, and nothing in the report said so.
    #[test]
    fn a_selected_but_unconsumed_promotion_is_stated_explicitly() {
        let entries = vec![
            selected(Analysis::PtrShape, "acc", "Ptr<Shape>"),
            selected(Analysis::PtrShape, "totals", "Ptr<Shape>"),
            with_outcome(Analysis::PtrShape, "acc", Outcome::Consumed, None),
            with_outcome(
                Analysis::PtrShape,
                "totals",
                Outcome::Unconsumed,
                Some("module_init_context"),
            ),
        ];
        let text = render_text(&entries);
        assert!(
            text.contains("Selected but NOT consumed (1)"),
            "the wasted promotion must have its own headline; got:\n{text}"
        );
        assert!(
            text.contains("dropped by module_init_context"),
            "the mechanism must be named, not just the count; got:\n{text}"
        );
        assert!(
            text.contains("1 of those selections were CONSUMED by codegen"),
            "the summary must separate selection from consumption; got:\n{text}"
        );
    }

    /// The worst case: every promotion of an instrumented analysis was wasted,
    /// and no mechanism was recorded either (the state a
    /// `PERRY_CANONICAL_I32_LOCALS=0` build produces, where the context gate is
    /// closed but the env-knob arm deliberately records no denial). The summary
    /// must still say `0 consumed`; staying silent renders it identically to an
    /// analysis nobody instrumented.
    #[test]
    fn a_fully_wasted_analysis_still_reports_zero_consumed() {
        let entries = vec![
            selected(Analysis::PtrShape, "acc", "Ptr<Shape>"),
            selected(Analysis::PtrShape, "totals", "Ptr<Shape>"),
        ];
        let text = render_text(&entries);
        assert!(
            text.contains("0 of those selections were CONSUMED by codegen"),
            "a fully wasted instrumented analysis must still print its zero; got:\n{text}"
        );
    }

    /// ...and an UNinstrumented analysis must stay silent, because a printed
    /// zero there would be a false claim that codegen applied nothing.
    #[test]
    fn an_uninstrumented_analysis_prints_no_consumption_line() {
        let entries = vec![selected(Analysis::CanonicalSlot, "i", "I32")];
        let text = render_text(&entries);
        assert!(
            !text.contains("CONSUMED by codegen"),
            "canonical-slot records no consumption; a zero there would be a lie; got:\n{text}"
        );
    }

    /// The compiler-side flag and the census's `CONSUMPTION_INSTRUMENTED` table
    /// are two spellings of one fact. Serializing the flag is what lets the
    /// census detect drift instead of silently trusting its own copy.
    #[test]
    fn json_exposes_which_analyses_record_consumption() {
        let json: serde_json::Value = serde_json::from_str(&render_json(&[selected(
            Analysis::PtrShape,
            "a",
            "Ptr<Shape>",
        )]))
        .unwrap();
        for row in json["summary"]["by_analysis"].as_array().unwrap() {
            let flag = row["records_consumption"]
                .as_bool()
                .unwrap_or_else(|| panic!("missing records_consumption on {row:?}"));
            assert_eq!(flag, row["analysis"] == "ptr-shape");
        }
    }

    /// A build where every promotion is applied must NOT grow the section —
    /// otherwise it is noise and stops being read.
    #[test]
    fn the_wasted_section_is_absent_when_everything_was_consumed() {
        let entries = vec![
            selected(Analysis::PtrShape, "acc", "Ptr<Shape>"),
            with_outcome(Analysis::PtrShape, "acc", Outcome::Consumed, None),
        ];
        let text = render_text(&entries);
        assert!(!text.contains("Selected but NOT consumed"), "got:\n{text}");
    }

    /// The census keys its consumed column off these fields. A report that
    /// counts consumption internally but does not SERIALIZE it leaves the
    /// census unable to tell a wasted promotion from an applied one — which is
    /// the entire failure being fixed.
    #[test]
    fn json_carries_consumed_and_unconsumed_per_analysis() {
        let entries = vec![
            selected(Analysis::PtrShape, "acc", "Ptr<Shape>"),
            selected(Analysis::PtrShape, "totals", "Ptr<Shape>"),
            with_outcome(Analysis::PtrShape, "acc", Outcome::Consumed, None),
            with_outcome(
                Analysis::PtrShape,
                "totals",
                Outcome::Unconsumed,
                Some("module_init_context"),
            ),
        ];
        let json: serde_json::Value = serde_json::from_str(&render_json(&entries)).unwrap();
        assert_eq!(json["summary"]["consumed"], 1);
        assert_eq!(json["summary"]["unconsumed"], 1);
        let row = json["summary"]["by_analysis"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["analysis"] == "ptr-shape")
            .expect("ptr-shape row");
        assert_eq!(row["selected"], 2, "selection keeps its old meaning");
        assert_eq!(row["consumed"], 1, "and no longer implies application");
        assert_eq!(row["unconsumed"], 1);
        // Every analysis must carry the field, so an uninstrumented one reads
        // as an explicit zero rather than an absent key.
        for a in json["summary"]["by_analysis"].as_array().unwrap() {
            assert!(a.get("consumed").is_some(), "missing consumed on {a:?}");
        }
    }

    /// `selected` is what a v1 consumer keyed performance claims off, and its
    /// meaning changed: it no longer implies emitted bytes. That is a schema
    /// break, not an additive field.
    #[test]
    fn splitting_selection_from_consumption_bumped_the_schema() {
        assert!(
            SCHEMA_VERSION >= 2,
            "the consumed/unconsumed split changes what `selected` means"
        );
    }

    /// Every analysis must render a distinct `analysis` key: the census keys
    /// its per-representation counts off this string, so a duplicate would
    /// silently merge two representations into one number — the aggregate
    /// that #7034 warns hides the interesting signal.
    #[test]
    fn analysis_keys_are_distinct() {
        let mut seen = std::collections::HashSet::new();
        for analysis in Analysis::ALL {
            assert!(
                seen.insert(analysis.as_str()),
                "duplicate analysis key {}",
                analysis.as_str()
            );
        }
    }

    /// The per-element column must be visible in text — it is the honest
    /// half of the hotness story (#7034 §8).
    #[test]
    fn per_element_context_is_rendered_separately_from_loop_depth() {
        let mut e = denied(Analysis::PtrShape, "rec", "rule 1 (provenance)");
        e.loop_depth = 0;
        e.invoked_per_element = Some("Array.prototype.map".into());
        let text = render_text(&[e]);
        assert!(
            text.contains("per-element callback of Array.prototype.map"),
            "got:\n{text}"
        );
    }
}

#[cfg(test)]
mod r0_bucket_tests {
    use super::*;
    use crate::opt_report::{Position, RegionKind};

    fn alloc_site(context: &str, rule: &str, tier: Tier, ordinal: u32) -> Entry {
        Entry {
            module: "dep.js".into(),
            function: "mk".into(),
            region: RegionKind::Function,
            position: Position::AllocSite,
            name: "object literal { ... }".into(),
            local_id: None,
            analysis: Analysis::PtrShape,
            outcome: Outcome::Denied,
            rep: "Boxed".into(),
            rule: Some(rule.into()),
            reason: Some("unbound".into()),
            tier: Some(tier),
            issue: None,
            loop_depth: 0,
            invoked_per_element: None,
            detail: Some(format!("allocation position: {context}")),
            byte_offset: None,
            site: None,
            alloc_context: Some(context.into()),
            alloc_ordinal: Some(ordinal),
        }
    }

    const RULE1: &str = "rule 1 (provenance)";
    const SERVED: &str = "rule 1 (provenance) — already served by return-shape";

    /// #7170 R0's primary deliverable, at the schema level: the bucket counts a
    /// scheduler reads are computed by the compiler that owns the rule
    /// vocabulary, not reconstructed with `jq` over `entries[]`. Two of the
    /// three defects R0 fixes were invisible to such a reduction by
    /// construction — it could not see that one label meant two mechanisms.
    #[test]
    fn json_reports_the_alloc_buckets_separately() {
        let entries = vec![
            alloc_site("constructor argument", RULE1, Tier::CompilerLimitation, 0),
            alloc_site(
                "object literal property value",
                RULE1,
                Tier::CompilerLimitation,
                1,
            ),
            alloc_site(
                "object literal property value",
                RULE1,
                Tier::CompilerLimitation,
                2,
            ),
            alloc_site("return", SERVED, Tier::Served, 3),
        ];
        let json: serde_json::Value = serde_json::from_str(&render_json(&entries)).unwrap();
        let buckets = json["summary"]["by_alloc_context"].as_array().unwrap();
        let find = |ctx: &str| {
            buckets
                .iter()
                .find(|b| b["context"] == ctx)
                .unwrap_or_else(|| panic!("missing {ctx} bucket in {buckets:#?}"))
                .clone()
        };
        assert_eq!(find("constructor argument")["denied"], 1);
        assert_eq!(find("object literal property value")["denied"], 2);
        assert_eq!(find("return")["denied"], 1);
        assert_eq!(find("return")["rule"], SERVED);
    }

    /// The wall number itself: `by_rule` must separate the served population
    /// from the rule-1 one, because "506 of 963" was a count of the merged
    /// bucket and every scheduling decision in the #7152/#7170 chain was taken
    /// off it.
    #[test]
    fn json_by_rule_separates_served_sites_from_the_rule_1_wall() {
        let entries = vec![
            alloc_site("return", RULE1, Tier::CompilerLimitation, 0),
            alloc_site("return", SERVED, Tier::Served, 1),
            alloc_site("return", SERVED, Tier::Served, 2),
        ];
        let json: serde_json::Value = serde_json::from_str(&render_json(&entries)).unwrap();
        let rules = json["summary"]["by_rule"].as_array().unwrap();
        let row = |rule: &str| {
            rules
                .iter()
                .find(|r| r["rule"] == rule)
                .unwrap_or_else(|| panic!("missing rule {rule} in {rules:#?}"))
                .clone()
        };
        assert_eq!(row(RULE1)["denied"], 1, "the wall is 1, not 3");
        assert_eq!(row(SERVED)["denied"], 2);
        assert_eq!(row(SERVED)["tier"], "served-elsewhere");
    }

    /// A served row must not be filed under the roadmap tier. That heading is
    /// what a scheduler reads as "Perry's outstanding work", and #7170 R0
    /// exists because already-shipped mechanisms were being counted there.
    #[test]
    fn a_served_site_renders_under_its_own_heading_not_the_roadmap_one() {
        let text = render_text(&[alloc_site("return", SERVED, Tier::Served, 0)]);
        assert!(
            text.contains("Already served by another mechanism"),
            "got:\n{text}"
        );
        assert!(
            !text.contains("Perry limitation (not your code)"),
            "a served row must not appear in the roadmap tier; got:\n{text}"
        );
    }

    /// Every tier must be renderable. The loop enumerates `Tier::ALL`, so a
    /// future tier cannot be added and silently left unprinted — which is how
    /// a row disappears from the human report while still being in the JSON.
    #[test]
    fn every_tier_has_a_heading_and_is_rendered() {
        for (i, tier) in Tier::ALL.into_iter().enumerate() {
            let text = render_text(&[alloc_site("return", RULE1, tier, i as u32)]);
            assert!(
                text.contains(tier.heading()),
                "{:?} is not rendered; got:\n{text}",
                tier
            );
        }
    }

    /// The allocation-position table is the human-readable half of the same
    /// fix, and it must state the position rather than leaving it to prose.
    #[test]
    fn text_reports_the_allocation_positions_as_a_table() {
        let text = render_text(&[
            alloc_site("constructor argument", RULE1, Tier::CompilerLimitation, 0),
            alloc_site(
                "object literal property value",
                RULE1,
                Tier::CompilerLimitation,
                1,
            ),
        ]);
        assert!(
            text.contains("Unbound allocation sites by position (2)"),
            "got:\n{text}"
        );
        assert!(
            text.contains("object literal property value"),
            "got:\n{text}"
        );
    }

    /// An entry that is not an allocation site contributes to `by_rule` and to
    /// nothing else — the alloc table must not grow rows for ordinary locals.
    #[test]
    fn a_local_denial_does_not_appear_in_the_alloc_table() {
        let mut local = alloc_site("return", RULE1, Tier::Fixable, 0);
        local.position = Position::Local;
        local.alloc_context = None;
        local.alloc_ordinal = None;
        let json: serde_json::Value = serde_json::from_str(&render_json(&[local])).unwrap();
        assert!(json["summary"]["by_alloc_context"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(json["summary"]["by_rule"].as_array().unwrap().len(), 1);
    }
}
