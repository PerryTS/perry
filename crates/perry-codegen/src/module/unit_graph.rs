//! The reference graph of a split module's functions, and the entry point
//! that partitions them (`unit_locality.rs` holds the partition itself).

use std::collections::HashMap;

use super::unit_locality::{
    locality_enabled, units_by_locality, units_by_size, UnitEdge, UnitNode,
};
use crate::function::LlFunction;

/// Call `sink` with every `@name` in `text` (unquoted names only: a function
/// this crate defines is never spelled quoted), and whether the name is the
/// callee of a call on that line.
fn for_each_symbol(text: &str, sink: &mut dyn FnMut(&str, bool)) {
    let b = text.as_bytes();
    let calls = text.contains("call ");
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != b'@' {
            i += 1;
            continue;
        }
        let start = i + 1;
        i = start;
        while i < b.len()
            && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'.' | b'$' | b'-'))
        {
            i += 1;
        }
        if i > start {
            sink(&text[start..i], calls && b.get(i) == Some(&b'('));
        }
    }
}

/// The references between `funcs`, read from their final instructions: the
/// same items the unit's LLVM module is built from, so a call the lowering
/// resolved (a typed `$sig` body, a specialized clone, a wrapper's target) is
/// a call here, and a call it left to the runtime is none.
pub(crate) fn reference_edges(funcs: &[&LlFunction]) -> Vec<UnitEdge> {
    use crate::function::FinalItem;
    use crate::inst::LlInst;
    let index: HashMap<&str, u32> = funcs
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.as_str(), i as u32))
        .collect();
    let mut edges: Vec<UnitEdge> = Vec::new();
    for (from, function) in funcs.iter().enumerate() {
        let from = from as u32;
        let first = edges.len();
        let mut note = |name: &str, direct: bool| {
            if let Some(&to) = index.get(name) {
                edges.push(UnitEdge { from, to, direct });
            }
        };
        let mut operand = |value: &str, note: &mut dyn FnMut(&str, bool)| {
            if value.contains('@') {
                for_each_symbol(value, &mut |name, _| note(name, false));
            }
        };
        function
            .for_each_final_item::<std::convert::Infallible>(&mut |item| {
                match item {
                    FinalItem::Inst(LlInst::Call { callee, args, .. }) => {
                        note(callee, true);
                        for (_, value) in args {
                            operand(value, &mut note);
                        }
                    }
                    FinalItem::Inst(LlInst::CallIndirect { args, .. }) => {
                        for (_, value) in args {
                            operand(value, &mut note);
                        }
                    }
                    FinalItem::Inst(LlInst::Store { val, .. }) => operand(val, &mut note),
                    FinalItem::Inst(LlInst::Cast { v, .. }) => operand(v, &mut note),
                    FinalItem::Inst(LlInst::Select { a, b, .. }) => {
                        operand(a, &mut note);
                        operand(b, &mut note);
                    }
                    FinalItem::Inst(LlInst::Phi { pairs, .. }) => {
                        for (value, _) in pairs {
                            operand(value, &mut note);
                        }
                    }
                    FinalItem::Inst(LlInst::Raw(line)) => {
                        if line.contains('@') {
                            for_each_symbol(line, &mut note);
                        }
                    }
                    FinalItem::Text(line) => {
                        if line.contains('@') {
                            for_each_symbol(line, &mut note);
                        }
                    }
                    FinalItem::Label(_) | FinalItem::Blank | FinalItem::Inst(_) => {}
                }
                Ok(())
            })
            .unwrap_or_else(|e| match e {});
        // One edge per (callee, kind) of this function.
        edges[first..].sort_unstable();
        let mut kept = first;
        for i in first..edges.len() {
            if i == first || edges[i] != edges[i - 1] {
                edges[kept] = edges[i];
                kept += 1;
            }
        }
        edges.truncate(kept);
    }
    edges
}

/// `PERRY_CODEGEN_UNIT_MAP=<dir>`: write `<dir>/<module>.units.tsv` for every
/// split module: an `F` row per function (definition index, unit, estimated
/// IR bytes, name) and an `E` row per reference (from, to, `call`/`ref`).
/// A report of the partition; nothing reads it back.
pub(crate) fn write_unit_map(
    module: &str,
    nodes: &[UnitNode<'_>],
    edges: &[UnitEdge],
    units: &[Vec<usize>],
) {
    let Some(dir) = std::env::var_os("PERRY_CODEGEN_UNIT_MAP") else {
        return;
    };
    let mut unit_of = vec![0usize; nodes.len()];
    for (u, unit) in units.iter().enumerate() {
        for &i in unit {
            unit_of[i] = u;
        }
    }
    let mut out = format!("# units={} functions={}\n", units.len(), nodes.len());
    for (i, node) in nodes.iter().enumerate() {
        out.push_str(&format!(
            "F\t{i}\t{}\t{}\t{}\n",
            unit_of[i], node.bytes, node.name
        ));
    }
    for e in edges {
        let kind = if e.direct { "call" } else { "ref" };
        out.push_str(&format!("E\t{}\t{}\t{kind}\n", e.from, e.to));
    }
    let module = if module.is_empty() { "module" } else { module };
    let _ = std::fs::write(
        std::path::Path::new(&dir).join(format!("{module}.units.tsv")),
        out,
    );
}

/// The partition of one split module, by the selected rule.
pub(crate) fn assign_units(
    module: &str,
    funcs: &[&LlFunction],
    sizes: &[usize],
    n: usize,
) -> Vec<Vec<usize>> {
    let locality = locality_enabled();
    let report = std::env::var_os("PERRY_CODEGEN_UNIT_MAP").is_some();
    if !locality && !report {
        return units_by_size(sizes, n);
    }
    let nodes: Vec<UnitNode<'_>> = funcs
        .iter()
        .zip(sizes)
        .map(|(f, &bytes)| UnitNode {
            name: &f.name,
            bytes,
        })
        .collect();
    let edges = reference_edges(funcs);
    let units = if locality {
        units_by_locality(&nodes, &edges, n)
    } else {
        units_by_size(sizes, n)
    };
    if report {
        write_unit_map(module, &nodes, &edges, &units);
    }
    units
}
