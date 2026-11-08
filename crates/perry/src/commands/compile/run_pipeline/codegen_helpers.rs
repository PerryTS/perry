//! Small codegen policies used by the compile orchestrator.

/// How many module codegen jobs may run at once: `PERRY_MODULE_JOBS` when set
/// to a positive count, else one per CPU slot of the compile's budget. Memory,
/// not this count, is what keeps large graphs in bounds (see
/// `module_schedule`).
pub(super) fn configured_module_codegen_threads(total_modules: usize, budget_cpus: usize) -> usize {
    std::env::var("PERRY_MODULE_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(budget_cpus)
        .clamp(1, total_modules.max(1))
}

#[cfg(test)]
mod module_codegen_job_tests {
    use super::configured_module_codegen_threads;

    #[test]
    fn one_job_per_cpu_slot_but_never_more_than_modules() {
        if std::env::var("PERRY_MODULE_JOBS").is_ok() {
            return;
        }
        assert_eq!(configured_module_codegen_threads(764, 48), 48);
        assert_eq!(configured_module_codegen_threads(3, 48), 3);
        assert_eq!(configured_module_codegen_threads(0, 48), 1);
        assert_eq!(configured_module_codegen_threads(10, 0), 1);
    }
}
/// Wrapper symbol used when a dynamic-import namespace materializes a local
/// function as a JavaScript value. Keep this routed through codegen's own
/// function mangler: module-name sanitization is intentionally not injective
/// for `$` and would point `$a` at `_a`'s symbol instead.
pub(super) fn namespace_local_function_wrapper_symbol(
    module_name: &str,
    function_name: &str,
) -> String {
    format!(
        "__perry_wrap_{}",
        perry_codegen::user_function_symbol(module_name, function_name)
    )
}
