//! Bound the copying caused by the raised guarded pre-statepoint budget.
//!
//! A body with a few top-level HIR statements can contain large nested loops.
//! The per-body admission limit alone also misses a small adapter whose early
//! inline callee is large. Inspect the existing lowered instructions directly;
//! no call-graph table, memoization, or emitted runtime state is needed.

use crate::{function::LlFunction, inst::LlInst, module::LlModule};

use super::{
    guarded_specialization_source_small_max_ir_bytes, GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES,
};

// A recursive or unusually deep unconditional-inline chain is not a bounded
// leaf. LLVM can still consider ordinary inlining after this admission fails.
const MAX_EXPANSION_DEPTH: usize = 16;

fn raw_callee(line: &str) -> Option<&str> {
    let line = line.trim();
    let call = line
        .strip_prefix("call ")
        .or_else(|| line.strip_prefix("invoke "))
        .or_else(|| line.split_once(" call ").map(|(_, rest)| rest))
        .or_else(|| line.split_once(" invoke ").map(|(_, rest)| rest))?;
    if call.contains(" noinline") || call.contains(" asm ") {
        return None;
    }
    let (_, after) = call.split_once('@')?;
    if let Some(quoted) = after.strip_prefix('"') {
        let (name, rest) = quoted.split_once('"')?;
        return rest.starts_with('(').then_some(name);
    }
    let (name, _) = after.split_once('(')?;
    // An indirect call can have a global-valued argument. That is not its
    // direct target; compiler-produced symbols do not contain whitespace.
    (!name.chars().any(char::is_whitespace)).then_some(name)
}

fn callees(function: &LlFunction) -> impl Iterator<Item = &str> {
    function
        .blocks()
        .iter()
        .flat_map(|block| block.insts())
        .filter_map(|inst| match inst {
            LlInst::Call {
                callee,
                no_inline: false,
                ..
            } => Some(callee.as_str()),
            LlInst::Raw(line) => raw_callee(line),
            _ => None,
        })
}

fn expanded_bytes(module: &LlModule, function: &LlFunction, depth: usize, limit: usize) -> usize {
    let mut bytes = function.estimated_ir_bytes();
    if depth == 0 || bytes > limit {
        return limit.saturating_add(1);
    }
    for name in callees(function) {
        let Some(callee) = module.function_named(name) else {
            continue;
        };
        if !callee.pre_statepoint_inline || callee.no_inline {
            continue;
        }
        bytes = bytes.saturating_add(expanded_bytes(module, callee, depth - 1, limit));
        if bytes > limit {
            return limit.saturating_add(1);
        }
    }
    bytes
}

fn within_growth_budget(module: &LlModule, function: &LlFunction) -> bool {
    let limit = guarded_specialization_source_small_max_ir_bytes()
        .max(GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES);
    let bytes = expanded_bytes(module, function, MAX_EXPANSION_DEPTH, limit);
    // Preserve the original compact-leaf allowance. The raised arm pays for
    // every known copy, instead of resetting the entire allowance per site.
    if bytes <= GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES {
        return true;
    }
    if bytes > limit {
        return false;
    }
    let max_sites = limit / bytes;
    let sites = module
        .diagnostic_functions()
        .flat_map(callees)
        .filter(|callee| *callee == function.name)
        .take(max_sites + 1)
        .count();
    sites <= max_sites
}

pub(in crate::codegen) fn apply(module: &mut LlModule) {
    // Removing an admission only decreases later expansion estimates. Thus
    // every retained body still satisfies the bound in the final graph. A
    // different declaration order may conservatively reject an additional
    // adapter; it cannot restore a rejected large callee or increase copying.
    for index in 0..module.function_count() {
        let Some(function) = module.diagnostic_functions().nth(index) else {
            continue;
        };
        if function.pre_statepoint_inline
            && function.guarded_preinline
            && !function.no_inline
            && !within_growth_budget(module, function)
        {
            module.function_mut(index).unwrap().pre_statepoint_inline = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DOUBLE;

    fn body(module: &mut LlModule, name: &str, size: usize) {
        let f = module.define_function(name, DOUBLE, vec![(DOUBLE, "%x".into())]);
        f.pre_statepoint_inline = true;
        f.guarded_preinline = true;
        let b = f.create_block("entry");
        for i in 0..size {
            b.emit_raw(format!("%n{i} = fadd double %x, 1.0"));
        }
        b.ret(DOUBLE, "%x");
    }

    fn caller(module: &mut LlModule, name: &str, callee: &str, count: usize) {
        let f = module.define_function(name, DOUBLE, vec![(DOUBLE, "%x".into())]);
        let b = f.create_block("entry");
        for _ in 0..count {
            b.call(DOUBLE, callee, &[(DOUBLE, "%x")]);
        }
        b.ret(DOUBLE, "%x");
    }

    fn admitted(module: &LlModule, name: &str) -> bool {
        module.function_named(name).unwrap().pre_statepoint_inline
    }

    #[test]
    fn large_body_budget_is_shared_by_all_direct_call_sites() {
        let mut module = LlModule::new("aarch64-apple-darwin");
        body(&mut module, "large", 1000);
        let bytes = module.function_named("large").unwrap().estimated_ir_bytes();
        assert!(bytes > GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES);
        assert!(bytes < guarded_specialization_source_small_max_ir_bytes());
        caller(&mut module, "caller", "large", 4);
        assert!(admitted(&module, "large"));
        apply(&mut module);
        assert!(!admitted(&module, "large"));
    }

    #[test]
    fn useful_single_copy_and_original_small_leaves_stay_admitted() {
        let mut module = LlModule::new("aarch64-apple-darwin");
        body(&mut module, "large", 1000);
        body(&mut module, "small", 5);
        caller(&mut module, "one", "large", 1);
        caller(&mut module, "many", "small", 100);
        apply(&mut module);
        assert!(admitted(&module, "large"));
        assert!(admitted(&module, "small"));
    }

    #[test]
    fn small_adapter_cannot_hide_large_transitive_copying() {
        let mut module = LlModule::new("aarch64-apple-darwin");
        body(&mut module, "large", 1000);
        caller(&mut module, "adapter", "large", 1);
        module.function_mut(1).unwrap().pre_statepoint_inline = true;
        module.function_mut(1).unwrap().guarded_preinline = true;
        caller(&mut module, "many", "adapter", 4);
        apply(&mut module);
        assert!(admitted(&module, "large"));
        assert!(!admitted(&module, "adapter"));
    }

    #[test]
    fn recursive_expansion_is_bounded_without_a_visited_table() {
        let mut module = LlModule::new("aarch64-apple-darwin");
        caller(&mut module, "recursive", "recursive", 1);
        module.function_mut(0).unwrap().pre_statepoint_inline = true;
        module.function_mut(0).unwrap().guarded_preinline = true;
        apply(&mut module);
        assert!(!admitted(&module, "recursive"));
    }

    #[test]
    fn declaration_order_never_reintroduces_large_transitive_copies() {
        for reverse in [false, true] {
            let mut module = LlModule::new("aarch64-apple-darwin");
            if reverse {
                caller(&mut module, "adapter", "large", 1);
                module.function_mut(0).unwrap().pre_statepoint_inline = true;
                module.function_mut(0).unwrap().guarded_preinline = true;
                body(&mut module, "large", 1000);
            } else {
                body(&mut module, "large", 1000);
                caller(&mut module, "adapter", "large", 1);
                module.function_mut(1).unwrap().pre_statepoint_inline = true;
                module.function_mut(1).unwrap().guarded_preinline = true;
            }
            caller(&mut module, "many", "adapter", 4);
            caller(&mut module, "also_large", "large", 4);
            apply(&mut module);
            assert!(!admitted(&module, "large"));
            for function in module.diagnostic_functions() {
                assert!(!function.pre_statepoint_inline || within_growth_budget(&module, function));
            }
            // The earlier adapter can be conservatively rejected while its
            // large callee was still admitted. The other order may retain
            // the now-small adapter, whose calls remain out of line.
            assert_eq!(admitted(&module, "adapter"), !reverse);
        }
    }

    #[test]
    fn textual_calls_include_invokes_but_not_indirect_global_arguments() {
        assert_eq!(
            raw_callee("%x = invoke double @leaf(double %a) to label %ok unwind label %bad"),
            Some("leaf")
        );
        assert_eq!(
            raw_callee("%x = call double @\"leaf$shape\"(double %a)"),
            Some("leaf$shape")
        );
        assert_eq!(
            raw_callee("%x = call double @leaf(double %a) noinline"),
            None
        );
        assert_eq!(raw_callee("%x = call double %fn(ptr @data)"), None);
    }

    #[test]
    fn independent_numeric_admission_is_preserved_but_its_expansion_is_charged() {
        let mut module = LlModule::new("aarch64-apple-darwin");
        body(&mut module, "numeric", 1000);
        module.function_mut(0).unwrap().guarded_preinline = false;
        caller(&mut module, "guarded_adapter", "numeric", 1);
        let adapter = module.function_mut(1).unwrap();
        adapter.pre_statepoint_inline = true;
        adapter.guarded_preinline = true;
        caller(&mut module, "many", "guarded_adapter", 4);
        caller(&mut module, "also_numeric", "numeric", 4);
        apply(&mut module);
        assert!(admitted(&module, "numeric"));
        assert!(!admitted(&module, "guarded_adapter"));
    }
}
