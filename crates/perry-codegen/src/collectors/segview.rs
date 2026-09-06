//! Segment-view for-of matcher — the fourth member of the escape-analysis
//! family (`escape_news`, `escape_arrays`, `escape_objects`).
//!
//! # What it looks for
//!
//! `for (let {segment: O} of X.segment(q))` — the loop `string-width` runs,
//! which the allocation census ranks 1/2/3 by count (172k records + 125k +
//! 123k substrings per 400-character claude-code reply, 58 % of the top-30
//! allocation count) and which the sample ranks as 60–85 % of active
//! main-thread CPU under ink's `wrapText`.
//!
//! After lowering, that loop is a fixed HIR shape (verified against
//! `--trace hir`, not assumed):
//!
//! ```text
//! Let { id: A, name: "__arr_A",    init: GetIterator(Call { PropertyGet(S, "segment"), [input] }) }
//! For {
//!   init:      Let { id: R, name: "__result_R", init: Call(ExternFuncRef "js_for_of_next", [LocalGet(A)]) },
//!   condition: Not(PropertyGet(LocalGet(R), "done")),
//!   update:    LocalSet(R, Call(ExternFuncRef "js_for_of_next", [LocalGet(A)])),
//!   body:      [ Let { id: D, name: "__destruct_D", init: PropertyGet(LocalGet(R), "value") },
//!                Let { id: O, name: <user>,         init: PropertyGet(LocalGet(D), "segment") },
//!                <user body> ]
//! }
//! ```
//!
//! # What it proves
//!
//! Only one thing, and it is the one the v1 runtime interface
//! (`INTERFACE_segments_view.md` §9b) needs: **the segment record `D` never
//! escapes**, because its every use is one of the destructuring field reads
//! that the loop head itself emits. When that holds the record is never
//! observed and never has to exist — census site 1, 172,032 allocations a
//! reply, all of it in the loop head.
//!
//! Whether the segment *substring* can also be skipped is a separate, weaker
//! question, and it is not a precondition: any use of `O` that no view entry
//! point answers is served by materialising the substring once into the same
//! local (`js_segments_view_segment`), which is exactly what the loop costs
//! today. So `O`'s uses are *classified and counted*, never fatal. The tally
//! is what says whether a site is also v2-ready (zero allocations per
//! grapheme) or only v1-ready (record elided, substring kept).
//!
//! # Soundness
//!
//! The escape proof is a *count*, and it is taken with
//! `perry_hir::collect_local_refs_stmt` — the repo's LocalId collector, which
//! handles every LocalId-bearing variant explicitly and delegates the rest to
//! `perry_hir::walker::walk_expr_children`, a match the compiler forces to be
//! exhaustive. A new HIR variant that embeds a `LocalGet` therefore cannot
//! silently hide a use of the record from this pass; it is a compile error in
//! the walker instead. That is the same reasoning `local_refs.rs`'s
//! `mark_all_candidate_refs_in_expr` catch-all exists for (#150), reached by
//! borrowing the sound walker rather than by re-deriving a conservative one.
//!
//! The `O`-use classifier is a hand-written recursive match, so it *can* fail
//! to recognise a shape — but it is checked against the same sound counter,
//! and every occurrence it did not classify is booked as `materialise`. An
//! unclassified use can therefore only make a site look *less* optimisable
//! than it is; it can never make one look more.
//!
//! # The counter is the falsifier
//!
//! A tier can be correct and never match (#9824). `PERRY_SEGVIEW_DIAG=1`
//! reports every for-of site examined, the verdict, and the rejection reason,
//! before codegen runs — so "it fires on the real bundle" is a measured line,
//! not an inference from the shape above.

use std::collections::HashMap;

use perry_hir::{Expr, Stmt, UnaryOp};

/// How each use of the destructured `segment` binding would be served.
///
/// Only `code_point_at` is answerable by a v1 runtime cursor
/// (`INTERFACE_segments_view.md` §9b); the rest are counted so the v1/v2 line
/// is measured rather than assumed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SegmentUseTally {
    /// `O.codePointAt(k)` — `js_segments_view_code_point_at`. v1.
    pub code_point_at: u32,
    /// `O.charCodeAt(k)` — v2 (`_char_code_at` was dropped from v1, §5).
    pub char_code_at: u32,
    /// `O.length` — v2 (`_length`, §5).
    pub length: u32,
    /// `RegExpTest { regex, string: LocalGet(O) }` — a *statically* proven
    /// regex receiver. v2 (`_regexp_test`).
    pub regexp_test_static: u32,
    /// `recv.test(O)` where `recv` is an arbitrary expression — cc's
    /// `g54.default().test(O)`. v2, and only behind the three-valued decline
    /// (§5): `is RegExp` at the call site does not rule out a patched
    /// `RegExp.prototype.test`.
    pub regexp_test_dynamic: u32,
    /// Everything else, including every occurrence the classifier did not
    /// recognise. Each one forces the substring to be materialised, which is
    /// what the loop pays today — never a rejection.
    pub materialise: u32,
}

impl SegmentUseTally {
    /// Total occurrences of the binding, from the sound counter.
    pub fn total(&self) -> u32 {
        self.code_point_at
            + self.char_code_at
            + self.length
            + self.regexp_test_static
            + self.regexp_test_dynamic
            + self.materialise
    }

    /// True when no use needs the substring: the loop reaches zero
    /// allocations per grapheme once the v2 entry points exist.
    pub fn view_answerable_v2(&self) -> bool {
        self.materialise == 0
    }

    /// True when every use is answered by the two in-loop v1 entry points.
    pub fn view_answerable_v1(&self) -> bool {
        self.total() == self.code_point_at
    }
}

/// Why a `for…of` whose subject is an `X.segment(q)` call did or did not
/// admit record elision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegViewVerdict {
    /// The record provably never escapes: every use is a destructuring read
    /// of `segment` in the loop head. v1 applies.
    Fires,
    /// The `For` head is not the canonical `js_for_of_next` protocol (a
    /// collection-view rewrite, an index arm, an async iterator…). Not a
    /// failure of the proof — a different lowering, which this tier does not
    /// speak.
    HeadNotCanonical,
    /// The head destructures no `segment` key, so this is somebody else's
    /// `.segment(x)`.
    NoSegmentKey,
    /// The record binding is boxed (captured by a closure that outlives the
    /// step). Rejected before the count, because a box read is not a
    /// `LocalGet` of the record.
    BoxedRecord,
    /// The record is used somewhere other than the head's own field reads.
    RecordEscapes {
        uses: usize,
        destructure_reads: usize,
    },
    /// The record does not escape, but the head reads fields v1 cannot answer
    /// (`index` / `input` / `isWordLike` are v2 symbols, §5). The spec path
    /// runs, at no cost, until those exist.
    RecordFieldsBeyondV1 { keys: Vec<String> },
}

impl SegViewVerdict {
    pub fn reason(&self) -> &'static str {
        match self {
            SegViewVerdict::Fires => "fires",
            SegViewVerdict::HeadNotCanonical => "head_not_canonical",
            SegViewVerdict::NoSegmentKey => "no_segment_key",
            SegViewVerdict::BoxedRecord => "boxed_record",
            SegViewVerdict::RecordEscapes { .. } => "record_escapes",
            SegViewVerdict::RecordFieldsBeyondV1 { .. } => "record_fields_beyond_v1",
        }
    }
}

/// One examined `for (… of X.segment(q))` site.
#[derive(Debug, Clone)]
pub struct SegmentForOfSite {
    /// `__arr_A`, the `GetIterator` local.
    pub iter_id: u32,
    /// `__result_R`, the iteration-result local.
    pub result_id: u32,
    /// `__destruct_D`, the segment record.
    pub record_id: u32,
    pub record_name: String,
    /// The local the `segment` key is destructured into, when there is one.
    pub segment_id: Option<u32>,
    /// The subject is the `X.segment(q)` call itself, so `open(segmenter,
    /// input)` — the two-argument form (§3) — is matchable and no `Segments`
    /// object is ever built. False for a for-of over a variable that already
    /// holds one, which takes the weaker `open_segments`.
    pub two_arg_open: bool,
    /// Field keys the head destructures off the record, in head order.
    pub record_keys: Vec<String>,
    /// Uses of `__arr_A` beyond the two `js_for_of_next` calls in the head —
    /// the iterator-close protocol (`it.return`) contributes 2. Reported
    /// because the lowering replaces `A` with a cursor and has to serve them.
    pub iter_extra_uses: usize,
    pub segment_uses: SegmentUseTally,
    pub verdict: SegViewVerdict,
}

impl SegmentForOfSite {
    pub fn fires(&self) -> bool {
        self.verdict == SegViewVerdict::Fires
    }
}

/// Collect every `for (… of X.segment(q))` site in one lowered region
/// (function body, method body, module init), including sites inside nested
/// closures.
///
/// Cheap by construction: the region is only walked at all when it contains a
/// `GetIterator` whose subject is a `.segment(…)` call.
pub fn collect_segment_for_of_sites(stmts: &[Stmt]) -> Vec<SegmentForOfSite> {
    let mut candidates: Vec<Candidate> = Vec::new();
    for_each_stmt_list(stmts, &mut |list| find_candidates_in_list(list, &mut candidates));
    // A statement list should be visited exactly once by `for_each_stmt_list`;
    // pin that rather than trusting it, so a descent bug shows up as a missing
    // site and never as a double-counted one.
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.iter_id));
    if candidates.is_empty() {
        return Vec::new();
    }

    // One sound reference census for the whole region, taken with the repo's
    // exhaustive LocalId collector. Multiset: a local referenced three times
    // contributes three entries.
    let mut refs: Vec<u32> = Vec::new();
    let mut visited = std::collections::HashSet::new();
    for stmt in stmts {
        perry_hir::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
    }
    let mut ref_counts: HashMap<u32, usize> = HashMap::new();
    for id in refs {
        *ref_counts.entry(id).or_insert(0) += 1;
    }

    // Boxed locals: a `Preallocate*`/`ReleaseBoxes` mention means the binding
    // lives in a heap cell a closure can reach, and a read of it is not a
    // `LocalGet` this census would see.
    let mut boxed: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for_each_stmt_list(stmts, &mut |list| {
        for s in list {
            match s {
                Stmt::PreallocateBoxes(ids)
                | Stmt::PreallocateTdzBoxes(ids)
                | Stmt::ReleaseBoxes(ids) => boxed.extend(ids.iter().copied()),
                _ => {}
            }
        }
    });

    candidates
        .into_iter()
        .map(|c| finish_candidate(c, &ref_counts, &boxed))
        .collect()
}

// ── candidate discovery ────────────────────────────────────────────────────

struct Candidate {
    iter_id: u32,
    result_id: u32,
    record_id: u32,
    record_name: String,
    segment_id: Option<u32>,
    two_arg_open: bool,
    record_keys: Vec<String>,
    head_canonical: bool,
    /// The whole `For` statement, for the `O`-use classification pass.
    for_stmt: Stmt,
}

fn find_candidates_in_list(list: &[Stmt], out: &mut Vec<Candidate>) {
    for (i, s) in list.iter().enumerate() {
        let Stmt::Let {
            id: iter_id,
            init: Some(Expr::GetIterator(subject)),
            ..
        } = s
        else {
            continue;
        };
        // The subject decides which `open` form the lowering can use, and
        // whether this is a segment loop at all.
        let two_arg_open = match subject.as_ref() {
            Expr::Call { callee, args, .. } => {
                matches!(callee.as_ref(), Expr::PropertyGet { property, .. } if property == "segment")
                    && args.len() == 1
            }
            _ => false,
        };
        if !two_arg_open {
            // A for-of over a variable already holding a `Segments` is the
            // weaker one-argument form. Nothing in the measured workload has
            // that shape, and matching it would need a type fact this pass
            // does not have, so it is not a candidate — and not a rejection
            // either, because it is not known to be a segment loop.
            continue;
        }
        // The `For` is the next statement, possibly inside a label.
        let Some(for_stmt) = next_for_stmt(list, i + 1) else {
            continue;
        };
        let Stmt::For {
            init,
            condition,
            update,
            body,
        } = for_stmt
        else {
            continue;
        };

        let head_canonical = head_is_canonical(*iter_id, init, condition, update);
        let (result_id, record_id, record_name, record_keys, segment_id) =
            match destructure_head(body) {
                Some(v) => v,
                None => continue,
            };

        out.push(Candidate {
            iter_id: *iter_id,
            result_id,
            record_id,
            record_name,
            segment_id,
            two_arg_open,
            record_keys,
            head_canonical,
            for_stmt: for_stmt.clone(),
        });
    }
}

fn next_for_stmt(list: &[Stmt], idx: usize) -> Option<&Stmt> {
    let mut s = list.get(idx)?;
    while let Stmt::Labeled { body, .. } = s {
        s = body.as_ref();
    }
    matches!(s, Stmt::For { .. }).then_some(s)
}

/// `init`/`condition`/`update` are the `js_for_of_next` protocol over
/// `iter_id`. Anything else is a different lowering (collection view, index
/// arm, async iterator) that this tier does not speak.
fn head_is_canonical(
    iter_id: u32,
    init: &Option<Box<Stmt>>,
    condition: &Option<Expr>,
    update: &Option<Expr>,
) -> bool {
    let Some(init) = init else { return false };
    let Stmt::Let {
        id: result_id,
        init: Some(init_expr),
        ..
    } = init.as_ref()
    else {
        return false;
    };
    if !is_for_of_next_call(init_expr, iter_id) {
        return false;
    }
    let done_ok = matches!(
        condition,
        Some(Expr::Unary { op: UnaryOp::Not, operand })
            if matches!(operand.as_ref(),
                Expr::PropertyGet { object, property, .. }
                    if property == "done" && matches!(object.as_ref(), Expr::LocalGet(r) if r == result_id))
    );
    let update_ok = matches!(
        update,
        Some(Expr::LocalSet(r, v)) if r == result_id && is_for_of_next_call(v, iter_id)
    );
    done_ok && update_ok
}

fn is_for_of_next_call(e: &Expr, iter_id: u32) -> bool {
    let Expr::Call { callee, args, .. } = e else {
        return false;
    };
    let Expr::ExternFuncRef { name, .. } = callee.as_ref() else {
        return false;
    };
    name == "js_for_of_next"
        && args.len() == 1
        && matches!(args[0], Expr::LocalGet(a) if a == iter_id)
}

/// Peel the leading destructuring reads the for-of head emits:
/// `Let D = result.value`, then one `Let = D.<key>` per destructured field.
type HeadShape = (u32, u32, String, Vec<String>, Option<u32>);

fn destructure_head(body: &[Stmt]) -> Option<HeadShape> {
    let Stmt::Let {
        id: record_id,
        name: record_name,
        init: Some(Expr::PropertyGet {
            object, property, ..
        }),
        ..
    } = body.first()?
    else {
        return None;
    };
    if property != "value" {
        return None;
    }
    let Expr::LocalGet(result_id) = object.as_ref() else {
        return None;
    };

    let mut keys = Vec::new();
    let mut segment_id = None;
    for s in body.iter().skip(1) {
        let Stmt::Let {
            id,
            init:
                Some(Expr::PropertyGet {
                    object, property, ..
                }),
            ..
        } = s
        else {
            break;
        };
        if !matches!(object.as_ref(), Expr::LocalGet(r) if r == record_id) {
            break;
        }
        if property == "segment" && segment_id.is_none() {
            segment_id = Some(*id);
        }
        keys.push(property.clone());
    }

    Some((
        *result_id,
        *record_id,
        record_name.clone(),
        keys,
        segment_id,
    ))
}

// ── the proof ──────────────────────────────────────────────────────────────

fn finish_candidate(
    c: Candidate,
    ref_counts: &HashMap<u32, usize>,
    boxed: &std::collections::HashSet<u32>,
) -> SegmentForOfSite {
    let record_uses = ref_counts.get(&c.record_id).copied().unwrap_or(0);
    let destructure_reads = c.record_keys.len();
    // The two `js_for_of_next(A)` calls the head itself emits.
    let iter_extra_uses = ref_counts
        .get(&c.iter_id)
        .copied()
        .unwrap_or(0)
        .saturating_sub(2);

    let mut segment_uses = SegmentUseTally::default();
    if let Some(seg_id) = c.segment_id {
        let sound_total = ref_counts.get(&seg_id).copied().unwrap_or(0) as u32;
        // One of those is the head's own `Let O = D.segment` init? No — that
        // is a use of the RECORD, not of `O`. Every counted reference of
        // `seg_id` is a real use in the body.
        classify_segment_uses_in_stmt(&c.for_stmt, seg_id, &mut segment_uses);
        // The classifier is hand-written and can miss a shape; the census
        // above cannot. Book the difference as "must materialise" so an
        // unrecognised use can only understate what the view buys.
        let classified = segment_uses.total();
        if sound_total > classified {
            segment_uses.materialise += sound_total - classified;
        }
    }

    let verdict = if !c.head_canonical {
        SegViewVerdict::HeadNotCanonical
    } else if c.segment_id.is_none() {
        SegViewVerdict::NoSegmentKey
    } else if boxed.contains(&c.record_id) {
        SegViewVerdict::BoxedRecord
    } else if record_uses != destructure_reads {
        SegViewVerdict::RecordEscapes {
            uses: record_uses,
            destructure_reads,
        }
    } else if c.record_keys.iter().any(|k| k != "segment") {
        SegViewVerdict::RecordFieldsBeyondV1 {
            keys: c.record_keys.clone(),
        }
    } else {
        SegViewVerdict::Fires
    };

    SegmentForOfSite {
        iter_id: c.iter_id,
        result_id: c.result_id,
        record_id: c.record_id,
        record_name: c.record_name,
        segment_id: c.segment_id,
        two_arg_open: c.two_arg_open,
        record_keys: c.record_keys,
        iter_extra_uses,
        segment_uses,
        verdict,
    }
}

// ── `O`-use classification ─────────────────────────────────────────────────

fn classify_segment_uses_in_stmt(stmt: &Stmt, seg: u32, t: &mut SegmentUseTally) {
    // Deep: every expression owned by `stmt` OR by any statement nested in it.
    // Closure bodies are reached from `classify_segment_uses_in_expr`'s own
    // `Expr::Closure` arm, which is the only path into them, so nothing is
    // visited twice.
    for_each_expr_in_stmt_shallow(stmt, &mut |e| classify_segment_uses_in_expr(e, seg, t));
    for_each_child_stmt(stmt, &mut |s| classify_segment_uses_in_stmt(s, seg, t));
}

/// Every statement nested directly inside `stmt` (branches, loop bodies,
/// catch/finally, switch cases, a `For` init). Does NOT enter closure bodies:
/// those hang off expressions and are handled by the expression classifier.
fn for_each_child_stmt(stmt: &Stmt, f: &mut impl FnMut(&Stmt)) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().for_each(&mut *f);
            if let Some(e) = else_branch {
                e.iter().for_each(&mut *f);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => body.iter().for_each(f),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                f(i);
            }
            body.iter().for_each(f);
        }
        Stmt::Labeled { body, .. } => f(body),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().for_each(&mut *f);
            if let Some(c) = catch {
                c.body.iter().for_each(&mut *f);
            }
            if let Some(fin) = finally {
                fin.iter().for_each(&mut *f);
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                c.body.iter().for_each(&mut *f);
            }
        }
        _ => {}
    }
}

fn classify_segment_uses_in_expr(e: &Expr, seg: u32, t: &mut SegmentUseTally) {
    // Recognise the parent shapes BEFORE descending, so the `LocalGet(seg)`
    // inside them is attributed rather than falling into `materialise`.
    match e {
        Expr::Call { callee, args, .. } => {
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                // `O.codePointAt(k)` / `O.charCodeAt(k)`
                if matches!(object.as_ref(), Expr::LocalGet(id) if *id == seg)
                    && args.len() == 1
                    && (property == "codePointAt" || property == "charCodeAt")
                {
                    if property == "codePointAt" {
                        t.code_point_at += 1;
                    } else {
                        t.char_code_at += 1;
                    }
                    for a in args {
                        classify_segment_uses_in_expr(a, seg, t);
                    }
                    return;
                }
                // `recv.test(O)` — the receiver is arbitrary (cc's
                // `g54.default()`), so the runtime decides per call.
                if property == "test"
                    && args.len() == 1
                    && matches!(&args[0], Expr::LocalGet(id) if *id == seg)
                    && !matches!(object.as_ref(), Expr::LocalGet(id) if *id == seg)
                {
                    t.regexp_test_dynamic += 1;
                    classify_segment_uses_in_expr(object, seg, t);
                    return;
                }
            }
        }
        Expr::RegExpTest { regex, string } => {
            if matches!(string.as_ref(), Expr::LocalGet(id) if *id == seg) {
                t.regexp_test_static += 1;
                classify_segment_uses_in_expr(regex, seg, t);
                return;
            }
        }
        Expr::PropertyGet {
            object, property, ..
        } => {
            if property == "length" && matches!(object.as_ref(), Expr::LocalGet(id) if *id == seg) {
                t.length += 1;
                return;
            }
        }
        Expr::LocalGet(id) if *id == seg => {
            t.materialise += 1;
            return;
        }
        Expr::Closure { body, .. } => {
            for s in body {
                classify_segment_uses_in_stmt(s, seg, t);
            }
            // Param defaults are Expr children; the walker below covers them.
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(e, &mut |child| {
        classify_segment_uses_in_expr(child, seg, t)
    });
}

// ── generic descent ────────────────────────────────────────────────────────

/// Call `f` on every statement list in the region, including the bodies of
/// nested closures. The `Stmt` arms are enumerated here; the `Expr` descent
/// that finds `Expr::Closure` delegates to the exhaustive walker.
fn for_each_stmt_list(stmts: &[Stmt], f: &mut impl FnMut(&[Stmt])) {
    f(stmts);
    for s in stmts {
        for_each_stmt_list_in_stmt(s, f);
    }
}

fn for_each_stmt_list_in_stmt(s: &Stmt, f: &mut impl FnMut(&[Stmt])) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for_each_stmt_list(then_branch, f);
            if let Some(e) = else_branch {
                for_each_stmt_list(e, f);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => for_each_stmt_list(body, f),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                for_each_stmt_list_in_stmt(i, f);
            }
            for_each_stmt_list(body, f);
        }
        Stmt::Labeled { body, .. } => for_each_stmt_list_in_stmt(body, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for_each_stmt_list(body, f);
            if let Some(c) = catch {
                for_each_stmt_list(&c.body, f);
            }
            if let Some(fin) = finally {
                for_each_stmt_list(fin, f);
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                for_each_stmt_list(&c.body, f);
            }
        }
        _ => {}
    }
    // Closure bodies hanging off any expression in this statement.
    for_each_expr_in_stmt_shallow(s, &mut |e| for_each_closure_body_in_expr(e, f));
}

fn for_each_closure_body_in_expr(e: &Expr, f: &mut impl FnMut(&[Stmt])) {
    if let Expr::Closure { body, .. } = e {
        for_each_stmt_list(body, f);
    }
    perry_hir::walker::walk_expr_children(e, &mut |child| for_each_closure_body_in_expr(child, f));
}

/// Every expression owned directly by `stmt` (not by its nested statements).
fn for_each_expr_in_stmt_shallow(stmt: &Stmt, f: &mut impl FnMut(&Expr)) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                f(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::Return(e) => {
            if let Some(e) = e {
                f(e);
            }
        }
        Stmt::If { condition, .. } => f(condition),
        Stmt::While { condition, .. } | Stmt::DoWhile { condition, .. } => f(condition),
        Stmt::For {
            init,
            condition,
            update,
            ..
        } => {
            if let Some(i) = init {
                for_each_expr_in_stmt_shallow(i, f);
            }
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
        }
        Stmt::Switch { discriminant, .. } => f(discriminant),
        Stmt::Labeled { body, .. } => for_each_expr_in_stmt_shallow(body, f),
        _ => {}
    }
}

// ── the counter ────────────────────────────────────────────────────────────
//
// A tier can be correct and never match (#9824), and this campaign has hit
// "exists in source, not helping the binary" five times. So the matcher ships
// with the instrument that decides whether it fires on the workload, and the
// instrument runs at the HIR-trace point — after every transform, on exactly
// the statements codegen consumes — which a 10 MB bundle reaches in minutes
// rather than the hours a full LLVM build costs.

/// `PERRY_SEGVIEW_DIAG=1`.
pub fn segview_diag_enabled() -> bool {
    matches!(std::env::var("PERRY_SEGVIEW_DIAG"), Ok(v) if !v.is_empty() && v != "0")
}

/// Every `Let _ = GetIterator(subject)` in a region, split by whether the
/// subject is an `X.segment(q)` call. The first number is the denominator
/// this tier is judged against: how many `for…of` loops exist at all.
pub fn count_for_of_sites(stmts: &[Stmt]) -> (usize, usize) {
    let (mut all, mut segment) = (0usize, 0usize);
    for_each_stmt_list(stmts, &mut |list| {
        for s in list {
            if let Stmt::Let {
                init: Some(Expr::GetIterator(subject)),
                ..
            } = s
            {
                all += 1;
                if let Expr::Call { callee, args, .. } = subject.as_ref() {
                    if args.len() == 1
                        && matches!(callee.as_ref(),
                            Expr::PropertyGet { property, .. } if property == "segment")
                    {
                        segment += 1;
                    }
                }
            }
        }
    });
    (all, segment)
}

/// Accumulated diagnostic over a whole compilation.
#[derive(Debug, Default)]
pub struct SegViewDiag {
    /// `for…of` sites of every kind (the denominator).
    pub for_of_sites: usize,
    /// Of those, sites whose subject is an `X.segment(q)` call.
    pub segment_subject_sites: usize,
    /// Every examined segment site, with the region it was found in.
    pub sites: Vec<(String, SegmentForOfSite)>,
}

impl SegViewDiag {
    pub fn scan_region(&mut self, region: &str, stmts: &[Stmt]) {
        let (all, seg) = count_for_of_sites(stmts);
        self.for_of_sites += all;
        self.segment_subject_sites += seg;
        if seg == 0 {
            return;
        }
        for site in collect_segment_for_of_sites(stmts) {
            self.sites.push((region.to_string(), site));
        }
    }

    /// Scan one lowered module: init statements, every free function, and
    /// every class constructor / method / accessor / static method. Sites
    /// inside a nested closure are attributed to the named region that
    /// encloses them, which is what a minified bundle gives us to name.
    pub fn scan_module(&mut self, path: &str, m: &perry_hir::Module) {
        self.scan_region(&format!("{path}::<init>"), &m.init);
        for f in &m.functions {
            self.scan_region(&format!("{path}::{}", f.name), &f.body);
        }
        for c in &m.classes {
            if let Some(ctor) = &c.constructor {
                self.scan_region(&format!("{path}::{}.constructor", c.name), &ctor.body);
            }
            for meth in c.methods.iter().chain(c.static_methods.iter()) {
                self.scan_region(&format!("{path}::{}.{}", c.name, meth.name), &meth.body);
            }
            for (name, f) in c.getters.iter().chain(c.setters.iter()) {
                self.scan_region(&format!("{path}::{}.{name}", c.name), &f.body);
            }
        }
    }

    /// Print the report to stderr. The lines are the falsifier: "fires=0" with
    /// a named reason is a result, "fires=0" with no reason is the failure
    /// mode this campaign keeps hitting.
    pub fn report(&self) {
        let mut fires = 0usize;
        let mut v1_only = 0usize;
        let mut v2_ready = 0usize;
        let mut by_reason: std::collections::BTreeMap<&'static str, usize> =
            std::collections::BTreeMap::new();

        for (region, s) in &self.sites {
            *by_reason.entry(s.verdict.reason()).or_insert(0) += 1;
            if s.fires() {
                fires += 1;
                if s.segment_uses.view_answerable_v2() {
                    v2_ready += 1;
                } else {
                    v1_only += 1;
                }
            }
            let u = &s.segment_uses;
            eprintln!(
                "[segview] {region} record={} (id={}) verdict={} open={} keys=[{}] \
                 iter_extra_uses={} O-uses: code_point_at={} char_code_at={} length={} \
                 regexp_test_static={} regexp_test_dynamic={} materialise={}",
                s.record_name,
                s.record_id,
                describe(&s.verdict),
                if s.two_arg_open { "2-arg" } else { "1-arg" },
                s.record_keys.join(","),
                s.iter_extra_uses,
                u.code_point_at,
                u.char_code_at,
                u.length,
                u.regexp_test_static,
                u.regexp_test_dynamic,
                u.materialise,
            );
        }

        eprintln!(
            "[segview] TOTALS for_of_sites={} segment_subject_sites={} examined={} fires={} \
             (v1_only={} v2_ready={})",
            self.for_of_sites,
            self.segment_subject_sites,
            self.sites.len(),
            fires,
            v1_only,
            v2_ready,
        );
        let reasons = by_reason
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("[segview] TOTALS verdicts: {reasons}");
    }
}

fn describe(v: &SegViewVerdict) -> String {
    match v {
        SegViewVerdict::RecordEscapes {
            uses,
            destructure_reads,
        } => format!("record_escapes(uses={uses},head_reads={destructure_reads})"),
        SegViewVerdict::RecordFieldsBeyondV1 { keys } => {
            format!("record_fields_beyond_v1({})", keys.join(","))
        }
        other => other.reason().to_string(),
    }
}
