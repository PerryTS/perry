//! Dominance of an emitted admission proof at a lowering continuation.
//!
//! Work on LLVM labels, including the inline invoke continuations inside a
//! builder block. A sibling visited earlier by the emitter is not an SSA
//! ancestor. No result is retained across changes to the partial CFG.
use super::LlFunction;
use crate::inst::LlInst;
use std::collections::{HashMap, HashSet};

impl LlFunction {
    /// Whether the existing i1 definition dominates the end of `use_block`.
    /// Unknown definitions and unreachable continuations refuse reuse.
    pub(crate) fn proof_dominates_block_end(&self, proof: &str, use_block: usize) -> bool {
        let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut definition = None;
        let mut use_label = None;
        for (index, block) in self.blocks.iter().enumerate() {
            let mut label = block.label.as_str();
            edges.entry(label).or_default();
            for inst in block.insts() {
                match inst {
                    LlInst::Bin { dst, .. }
                    | LlInst::ICmp { dst, .. }
                    | LlInst::Phi { dst, .. }
                        if dst == proof =>
                    {
                        definition = Some(label)
                    }
                    LlInst::Br { label: target } => {
                        edges.entry(label).or_default().push(target);
                    }
                    LlInst::CondBr { t, f, .. } => {
                        edges
                            .entry(label)
                            .or_default()
                            .extend([t.as_str(), f.as_str()]);
                    }
                    LlInst::Raw(text) => {
                        for line in text.lines().map(str::trim) {
                            if let Some(next) = line.strip_suffix(':') {
                                label = next;
                                edges.entry(label).or_default();
                                continue;
                            }
                            if let Some((dst, _)) = line.split_once(" = ") {
                                if dst == proof {
                                    definition = Some(label);
                                }
                            }
                            // Raw invoke, switch and br edges; phi predecessor
                            // labels never contain `label %`.
                            for target in line.split("label %").skip(1) {
                                let target = target
                                    .split(|c: char| c.is_whitespace() || matches!(c, ',' | ']'))
                                    .next()
                                    .unwrap_or("");
                                edges.entry(label).or_default().push(target);
                            }
                        }
                    }
                    _ => {}
                }
            }
            if index == use_block {
                use_label = Some(label);
            }
        }
        let (Some(definition), Some(use_label), Some(entry)) =
            (definition, use_label, self.blocks.first())
        else {
            return false;
        };
        if definition == use_label {
            return true;
        }
        let reachable = |skip: Option<&str>| {
            let mut pending = vec![entry.label.as_str()];
            let mut seen = HashSet::new();
            while let Some(label) = pending.pop() {
                if Some(label) == skip || !seen.insert(label) {
                    continue;
                }
                if label == use_label {
                    return true;
                }
                pending.extend(edges.get(label).into_iter().flatten().copied());
            }
            false
        };
        reachable(None) && !reachable(Some(definition))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{I1, VOID};

    #[test]
    fn family_run_proof_refuses_siblings_and_bypassing_joins() {
        let mut f = LlFunction::new("probe", VOID, vec![]);
        f.create_block("entry");
        f.create_block("then");
        f.create_block("else");
        f.create_block("join");
        let labels: Vec<_> = f.blocks().iter().map(|b| b.label.clone()).collect();
        f.block_mut(0)
            .unwrap()
            .cond_br("true", &labels[1], &labels[2]);
        let proof = f.block_mut(1).unwrap().icmp_eq(I1, "true", "true");
        f.block_mut(1).unwrap().br(&labels[3]);
        f.block_mut(2).unwrap().br(&labels[3]);
        assert!(f.proof_dominates_block_end(&proof, 1));
        assert!(!f.proof_dominates_block_end(&proof, 2));
        assert!(!f.proof_dominates_block_end(&proof, 3));
        let joined = f
            .block_mut(3)
            .unwrap()
            .phi(I1, &[(&proof, &labels[1]), ("false", &labels[2])]);
        assert!(f.proof_dominates_block_end(&joined, 3));
        assert!(!f.proof_dominates_block_end("%missing", 3));
    }

    #[test]
    fn family_run_proof_handles_diamonds_loops_and_inline_invoke_labels() {
        let mut f = LlFunction::new("probe", VOID, vec![]);
        f.create_block("entry");
        f.create_block("body");
        f.create_block("exit");
        let labels: Vec<_> = f.blocks().iter().map(|b| b.label.clone()).collect();
        let proof = f.block_mut(0).unwrap().icmp_eq(I1, "true", "true");
        f.block_mut(0).unwrap().br(&labels[1]);
        f.block_mut(1)
            .unwrap()
            .cond_br("true", &labels[1], &labels[2]);
        assert!(f.proof_dominates_block_end(&proof, 1));
        assert!(f.proof_dominates_block_end(&proof, 2));
        // The invoke's unwind bypasses the definition in its normal continuation.
        f.block_mut(2)
            .unwrap()
            .emit_raw("  invoke void @foo() to label %normal unwind label %caught");
        f.block_mut(2).unwrap().emit_raw("normal:");
        let normal = f.block_mut(2).unwrap().icmp_eq(I1, "true", "true");
        f.create_block("caught");
        // Builder labels carry a suffix; use an inline label to model EH exactly.
        f.block_mut(3).unwrap().emit_raw("caught:");
        assert!(!f.proof_dominates_block_end(&normal, 3));
        assert!(f.proof_dominates_block_end(&proof, 3));
    }

    #[cfg(feature = "llvm-inprocess")]
    #[test]
    fn family_run_proof_constructor_branches_verify_before_llvm_codegen() {
        // PostCSS Input's BOM branch: the second store in then re-arms
        // the run, and the first store in else must not inherit its proof.
        let source = r#"
            class Input {
                constructor(css) {
                    this.css = css.toString();
                    if (this.css === "bom") {
                        this.hasBOM = true;
                        this.css = this.css.toString();
                    } else {
                        this.hasBOM = false;
                    }
                    this.document = this.css;
                }
            }
            const input = new Input("plain");
            console.log(input.document, input.hasBOM);
        "#;
        let ast = perry_parser::parse_typescript(source, "family_run.ts").unwrap();
        let hir = perry_hir::lower_module(&ast, "family_run", "family_run.ts").unwrap();
        let mut opts = crate::temp_root_coverage::entry_opts();
        opts.program_layouts = std::sync::Arc::new(crate::layout_families::compute(
            [("family_run.ts", &hir)],
            true,
            &Default::default(),
        ));
        assert!(
            !opts.program_layouts.families.is_empty(),
            "fixture must exercise family stores"
        );
        let ir = String::from_utf8(crate::compile_module(&hir, opts).unwrap()).unwrap();
        assert!(
            ir.contains("crun.join"),
            "fixture must emit the admission phi"
        );
        assert!(
            ir.contains("l2i.gate"),
            "fixture must consume a family proof"
        );
        crate::testing::verify_ir(&ir, "family_run").unwrap();
    }
}
