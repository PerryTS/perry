//! Reuse a root reload across a stretch that cannot collect.
//!
//! Every read of a root slot is lowered to a load plus the
//! `ROOT_RELOAD_LAUNDER` identity (#9499), and every launder output is a
//! distinct opaque integer. Two reads of `this` in one loop body therefore
//! produce two unrelated bases as far as LLVM can tell: a store through the
//! first does not forward to a load through the second, the address arithmetic
//! is redone per access, and nothing derived from either can leave the loop.
//! On `this.rights &= a; this.rights |= b` that is a store, a reload of
//! `this`, a reload of the field and a second store per iteration.
//!
//! The launder is needed only where the slot's value can differ from the last
//! read: after a safepoint (a moving collection may have rewritten the slot)
//! or after a store to the slot. Between two reads with neither on ANY path
//! in between, the second read returns the first's bits, so it may name the
//! first's laundered integer instead of minting a new one.
//!
//! Soundness does not weaken the barrier:
//!
//! - What is reused is the launder's OUTPUT, an ordinary `i64`, never the
//!   `ptr addrspace(1)` it came from. No GC reference becomes the operand of
//!   two safepoints (the #9499 failure); there is no safepoint between the two
//!   reads at all.
//! - "No safepoint" is decided per path by a forward must-availability
//!   analysis (intersection at joins), so the reused value dominates the read
//!   it replaces and no path from it reaches the read through a collecting
//!   call. A loop back edge carrying a poll kills availability at the header.
//! - A call is a safepoint unless it carries `"gc-leaf-function"` at the call
//!   site (the parent lowering marks exactly the `gc_call_effects`
//!   `CannotCollect` callees and inline asm that way) or is a pure LLVM
//!   intrinsic from a short allowlist. Anything else, including every
//!   `invoke` that is not marked leaf, kills every availability.
//!
//! `PERRY_UNBOXED_CLASS_FIELDS=0` turns the pass off.
//! `PERRY_UNBOXED_CLASS_FIELDS=sabotage` is the test-only negative control:
//! it treats every call as unable to collect, i.e. it reuses reloads ACROSS
//! safepoints, which is exactly the stale-pointer bug the analysis exists to
//! prevent. `test-parity/language/class/test_class_field_loop_gc_pressure.ts`
//! must fail under it.

use std::collections::HashMap;

/// LLVM intrinsics that never lower to a call into the runtime. Anything with
/// `llvm.` not listed here is treated as a safepoint.
const PURE_INTRINSIC_PREFIXES: &[&str] = &[
    "llvm.aarch64.fjcvtzs",
    "llvm.read_register",
    "llvm.fabs.",
    "llvm.copysign.",
    "llvm.minnum.",
    "llvm.maxnum.",
    "llvm.minimum.",
    "llvm.maximum.",
    "llvm.sqrt.",
    "llvm.floor.",
    "llvm.ceil.",
    "llvm.trunc.",
    "llvm.rint.",
    "llvm.nearbyint.",
    "llvm.round.",
    "llvm.roundeven.",
    "llvm.ctlz.",
    "llvm.cttz.",
    "llvm.ctpop.",
    "llvm.bswap.",
    "llvm.bitreverse.",
    "llvm.fshl.",
    "llvm.fshr.",
    "llvm.smax.",
    "llvm.smin.",
    "llvm.umax.",
    "llvm.umin.",
    "llvm.abs.",
    "llvm.sadd.with.overflow.",
    "llvm.ssub.with.overflow.",
    "llvm.smul.with.overflow.",
    "llvm.uadd.with.overflow.",
    "llvm.usub.with.overflow.",
    "llvm.umul.with.overflow.",
    "llvm.fptosi.sat.",
    "llvm.fptoui.sat.",
    "llvm.expect.",
    "llvm.assume",
    "llvm.lifetime.",
    "llvm.prefetch",
];

/// `PERRY_UNBOXED_CLASS_FIELDS` value -> whether the pass runs.
fn switch_allows(value: Option<&str>) -> bool {
    !matches!(value, Some("0") | Some("off") | Some("false"))
}

pub(super) fn enabled() -> bool {
    switch_allows(std::env::var("PERRY_UNBOXED_CLASS_FIELDS").ok().as_deref())
}

/// `(X, R)` when `line` is the parent lowering's reload head
/// `X.rs4p = load ptr addrspace(1), ptr R`.
fn reload_head(line: &str) -> Option<(&str, &str)> {
    let (lhs, ptr) = line.trim().split_once(" = load ptr addrspace(1), ptr ")?;
    let result = lhs.strip_suffix(".rs4p")?;
    (!ptr.contains(' ') && !ptr.contains(',')).then_some((result, ptr))
}

/// Whether `line` is the launder half of the reload of `result`.
fn is_launder_of(line: &str, result: &str) -> bool {
    let t = line.trim();
    t.strip_prefix(result)
        .and_then(|r| r.strip_prefix(".rs4o = call i64 asm \"\", \"=r,0\"(ptr addrspace(1) "))
        .and_then(|r| r.strip_prefix(result))
        .is_some_and(|r| r.starts_with(".rs4p)"))
}

/// The root slot a `store ptr addrspace(1) ..., ptr R` writes, if any.
fn root_store(line: &str) -> Option<&str> {
    let t = line.trim();
    if !t.starts_with("store ptr addrspace(1) ") {
        return None;
    }
    Some(t.rsplit_once(", ptr ")?.1.trim())
}

fn is_label(line: &str) -> Option<&str> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let head = line.split(';').next()?.trim_end();
    let name = head.strip_suffix(':')?;
    (!name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$' | '-')))
    .then_some(name)
}

/// The `.rs4o` whose bits `store ptr addrspace(1) %s, ptr R` writes, when `%s`
/// is the parent lowering's re-root of a re-read:
/// `%s = inttoptr i64 %b`, `%b = bitcast double %v to i64`,
/// `%v = bitcast i64 %x.rs4o to double`.
fn stored_rs4o(defs: &HashMap<&str, &str>, line: &str) -> Option<String> {
    let value = line
        .trim()
        .strip_prefix("store ptr addrspace(1) ")?
        .rsplit_once(", ptr ")?
        .0
        .trim();
    let b = defs
        .get(value)?
        .strip_prefix("inttoptr i64 ")?
        .strip_suffix(" to ptr addrspace(1)")?;
    let v = defs
        .get(b)?
        .strip_prefix("bitcast double ")?
        .strip_suffix(" to i64")?;
    let x = defs
        .get(v)?
        .strip_prefix("bitcast i64 ")?
        .strip_suffix(" to double")?;
    x.ends_with(".rs4o").then(|| x.to_string())
}

/// Could executing `line` reach a safepoint?
fn may_collect(line: &str) -> bool {
    let t = line.trim();
    let is_call = t.starts_with("call ")
        || t.starts_with("tail call ")
        || t.starts_with("musttail call ")
        || t.starts_with("notail call ")
        || t.contains(" = call ")
        || t.contains(" = tail call ")
        || t.contains(" = musttail call ")
        || t.contains(" = notail call ");
    let is_invoke = t.starts_with("invoke ") || t.contains(" = invoke ");
    if !is_call && !is_invoke {
        return false;
    }
    if t.contains("\"gc-leaf-function\"") {
        return false;
    }
    if is_call {
        if let Some(callee) = t
            .split_once('@')
            .map(|(_, rest)| rest.split('(').next().unwrap_or(""))
        {
            if PURE_INTRINSIC_PREFIXES
                .iter()
                .any(|p| callee.starts_with(p))
            {
                return false;
            }
        }
    }
    true
}

fn successors(line: &str, out: &mut Vec<String>) {
    let mut rest = line;
    while let Some(at) = rest.find("label %") {
        let tail = &rest[at + "label %".len()..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$' | '-'))
            .collect();
        if !name.is_empty() {
            out.push(name);
        }
        rest = tail;
    }
}

type Avail = HashMap<String, String>;

struct Block {
    start: usize,
    end: usize,
    succs: Vec<usize>,
}

fn meet(into: &mut Option<Avail>, from: &Avail) -> bool {
    match into {
        None => {
            *into = Some(from.clone());
            true
        }
        Some(cur) => {
            let before = cur.len();
            cur.retain(|k, v| from.get(k) == Some(v));
            cur.len() != before
        }
    }
}

/// Walk one block's lines from `state`, returning the exit state. `emit`
/// receives each line and, for a reload whose value is available, the name to
/// reuse.
fn walk(
    lines: &[&str],
    defs: &HashMap<&str, &str>,
    block: &Block,
    sabotage: bool,
    mut state: Avail,
    mut emit: impl FnMut(usize, Option<&str>),
) -> Avail {
    let mut i = block.start;
    while i < block.end {
        let line = lines[i];
        if let Some((result, root)) = reload_head(line) {
            if i + 1 < block.end && is_launder_of(lines[i + 1], result) {
                if let Some(prev) = state.get(root) {
                    emit(i, Some(prev.as_str()));
                } else {
                    state.insert(root.to_string(), format!("{result}.rs4o"));
                    emit(i, None);
                    emit(i + 1, None);
                }
                i += 2;
                continue;
            }
            // An unrecognised read of the slot: forget it.
            state.remove(root);
        } else if let Some(root) = root_store(line) {
            // A store of a re-read's own bits (`bitcast` chain back to an
            // `.rs4o`) leaves the slot holding exactly those bits until the
            // next safepoint, so a later read of it may name them too.
            match stored_rs4o(defs, line) {
                Some(src) => {
                    state.insert(root.to_string(), src);
                }
                None => {
                    state.remove(root);
                }
            }
        } else if !sabotage && may_collect(line) {
            state.clear();
        }
        emit(i, None);
        i += 1;
    }
    state
}

/// Rewrite one lowered function body. Returns the input unchanged when the
/// pass is off or finds nothing to reuse.
pub(in crate::function) fn reuse_root_reloads(ir: &str) -> String {
    if !enabled() {
        return ir.to_string();
    }
    let sabotage = std::env::var("PERRY_UNBOXED_CLASS_FIELDS").as_deref() == Ok("sabotage");
    reuse_with(ir, sabotage)
}

#[cfg(test)]
fn reuse(ir: &str) -> String {
    reuse_with(ir, false)
}

fn reuse_with(ir: &str, sabotage: bool) -> String {
    if !ir.contains(".rs4p = load ptr addrspace(1)") {
        return ir.to_string();
    }
    let lines: Vec<&str> = ir.lines().collect();
    let defs: HashMap<&str, &str> = lines
        .iter()
        .filter_map(|l| l.trim().split_once(" = "))
        .filter(|(lhs, _)| lhs.starts_with('%') && !lhs.contains(' '))
        .collect();
    // Blocks: the text before the first label is the entry block.
    let mut blocks: Vec<Block> = Vec::new();
    let mut names: HashMap<&str, usize> = HashMap::new();
    let mut start = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if let Some(name) = is_label(line) {
            if i > start || blocks.is_empty() {
                blocks.push(Block {
                    start,
                    end: i,
                    succs: Vec::new(),
                });
            }
            names.insert(name, blocks.len());
            start = i + 1;
        }
    }
    blocks.push(Block {
        start,
        end: lines.len(),
        succs: Vec::new(),
    });
    for b in &mut blocks {
        let mut succ_names = Vec::new();
        for line in &lines[b.start..b.end] {
            successors(line, &mut succ_names);
        }
        let mut succs: Vec<usize> = Vec::new();
        for n in succ_names {
            match names.get(n.as_str()) {
                Some(&idx) => succs.push(idx),
                // An edge we cannot resolve: refuse the whole function.
                None => return ir.to_string(),
            }
        }
        b.succs = succs;
    }

    // Forward must-availability to a fixpoint. `None` = not yet reached.
    // The function's entry: the first labeled block (the `define` line's
    // pseudo-block falls into it without an edge), or the pseudo-block itself
    // when the body has no labels. A block nothing branches to is otherwise
    // unreachable — the dead arm of a branch folded at emission — and stays
    // unvisited, so it cannot weaken a join it never reaches.
    let entry_block = if blocks.len() > 1 { 1 } else { 0 };
    let mut entry: Vec<Option<Avail>> = vec![None; blocks.len()];
    let mut work: Vec<usize> = vec![entry_block];
    let mut queued = vec![false; blocks.len()];
    entry[entry_block] = Some(Avail::new());
    queued[entry_block] = true;
    let mut steps = 0usize;
    while let Some(b) = work.pop() {
        queued[b] = false;
        steps += 1;
        if steps > blocks.len().saturating_mul(64).max(4096) {
            // Pathological CFG: keep the original lowering.
            return ir.to_string();
        }
        let state = entry[b].clone().unwrap_or_default();
        let exit = walk(&lines, &defs, &blocks[b], sabotage, state, |_, _| {});
        for &s in &blocks[b].succs {
            if meet(&mut entry[s], &exit) && !queued[s] {
                queued[s] = true;
                work.push(s);
            }
        }
    }

    let mut out = String::with_capacity(ir.len());
    let mut reused = 0usize;
    let mut cursor = 0usize;
    for (bi, block) in blocks.iter().enumerate() {
        while cursor < block.start {
            out.push_str(lines[cursor]);
            out.push('\n');
            cursor += 1;
        }
        let state = entry[bi].clone().unwrap_or_default();
        walk(
            &lines,
            &defs,
            block,
            sabotage,
            state,
            |i, prev| match prev {
                Some(prev) => {
                    // `walk` already consumed the launder line after this head.
                    let (result, _) = reload_head(lines[i]).expect("reload head");
                    out.push_str(&format!("  {result}.rs4o = add i64 {prev}, 0\n"));
                    reused += 1;
                }
                None => {
                    out.push_str(lines[i]);
                    out.push('\n');
                }
            },
        );
        cursor = block.end;
    }
    while cursor < lines.len() {
        out.push_str(lines[cursor]);
        out.push('\n');
        cursor += 1;
    }
    if reused == 0 {
        return ir.to_string();
    }
    out
}

#[cfg(test)]
#[path = "reload_reuse_tests.rs"]
mod tests;
