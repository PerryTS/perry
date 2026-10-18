//! Parallel lowering keeps module authorities on the calling thread. Each
//! worker owns an empty module and literal pool; owned output groups are
//! merged in the same order as the serial loop, irrespective of completion.
use super::{helpers, static_constfn, static_shape_ids};
use crate::{module::LlModule, strings::StringPool};
use anyhow::Result;

struct State {
    #[cfg(test)]
    census: Option<perry_hir::ProtoCensus>,
    target: helpers::CallableTargetState,
    constfn: static_constfn::CallableState,
    shapes: static_shape_ids::CallableState,
    layouts: crate::layout_desc::CallableState,
    creates: crate::layout_desc::create_families::CallableState,
    globals: std::collections::HashSet<u32>,
    executable: bool,
}
impl State {
    fn capture() -> Self {
        Self {
            #[cfg(test)]
            census: crate::proto_constants::callable_test_census(),
            target: helpers::callable_target_state(),
            constfn: static_constfn::callable_state(),
            shapes: static_shape_ids::callable_state(),
            layouts: crate::layout_desc::callable_state(),
            creates: crate::layout_desc::create_families::callable_state(),
            globals: crate::stmt::region_loop::callable_const_globals(),
            executable: crate::expr::agent_ptr::output_is_executable(),
        }
    }
    fn install(&self) {
        #[cfg(test)]
        crate::proto_constants::install_callable_test_census(self.census.clone());
        self.target.install();
        self.constfn.install();
        self.shapes.install();
        self.layouts.install();
        self.creates.install();
        crate::stmt::region_loop::install_callable_const_globals(self.globals.clone());
        crate::expr::agent_ptr::set_output_is_executable(self.executable);
        crate::ext_registry::begin_module_capture();
    }
}
struct Effects {
    shapes: static_shape_ids::CallableEffects,
    layouts: crate::layout_desc::CallableEffects,
    creates: std::collections::BTreeMap<String, crate::layout_desc::create_families::Site>,
    ffi: Vec<&'static str>,
}
impl Effects {
    fn take() -> Self {
        Self {
            shapes: static_shape_ids::take_callable_effects(),
            layouts: crate::layout_desc::take_callable_effects(),
            creates: crate::layout_desc::create_families::take_callable_effects(),
            ffi: crate::ext_registry::take_module_capture(),
        }
    }
    fn merge(self) {
        self.shapes.merge();
        self.layouts.merge();
        crate::layout_desc::create_families::merge_callable_effects(self.creates);
        crate::ext_registry::merge_callable_capture(self.ffi);
    }
}

/// Explicit jobs=1 exercises the original serial path. Profiles that choose
/// code by serial site id retain that path, as do attribution diagnostics.
pub(super) fn lower<T: Sync>(
    module: &mut LlModule,
    strings: &mut StringPool,
    items: &[T],
    parallel: bool,
    lower: impl Fn(&mut LlModule, &mut StringPool, &T) -> Result<()> + Sync,
    mut progress: impl FnMut(usize),
) -> Result<()> {
    let jobs = std::env::var("PERRY_LOWERING_JOBS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| crate::workers::unit_workers().min(4));
    #[cfg(test)]
    let jobs = TEST_JOBS.with(|s| s.get()).unwrap_or(jobs);
    let diagnostic = crate::opt_report::enabled()
        || crate::typed_feedback_profile::callable_scope_active()
        || std::env::var_os("PERRY_REGION_DIAG").is_some()
        || crate::fallback_diag::enabled();
    if !parallel || jobs <= 1 || items.len() < 64 || diagnostic {
        for (i, item) in items.iter().enumerate() {
            lower(module, strings, item)?;
            progress(i + 1);
        }
        return Ok(());
    }
    let budget = crate::workers::current_budget();
    let mut grants = Vec::new();
    let mut count = 1;
    while count < jobs.min(items.len()) {
        if let Some(budget) = &budget {
            let Some(grant) = budget.try_acquire(512 << 20) else {
                break;
            };
            grants.push(grant);
        }
        count += 1;
    }
    if count == 1 {
        for (i, item) in items.iter().enumerate() {
            lower(module, strings, item)?;
            progress(i + 1);
        }
        return Ok(());
    }
    let input = module.callable_input();
    let pool = strings.callable_config();
    let state = State::capture();
    std::thread::scope(|scope| -> Result<()> {
        let mut handles = Vec::new();
        let chunk_size = items.len().div_ceil(count);
        for chunk in items.chunks(chunk_size) {
            let (input, pool, state, lower) = (&input, &pool, &state, &lower);
            handles.push((
                chunk.len(),
                std::thread::Builder::new()
                    .name(format!("perry-lower-{}", handles.len()))
                    .stack_size(64 * 1024 * 1024)
                    .spawn_scoped(scope, move || -> Result<_> {
                        state.install();
                        let mut module = input.module();
                        let mut strings = pool.pool();
                        for item in chunk {
                            lower(&mut module, &mut strings, item)?;
                        }
                        Ok((module.into_callable_output(), strings, Effects::take()))
                    })?,
            ));
        }
        let mut done = 0;
        for (len, handle) in handles {
            let (output, pool, effects) = handle
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))?;
            let symbols = strings.merge_callable(pool);
            module.merge_callable(output, &input, symbols);
            effects.merge();
            done += len;
            progress(done);
        }
        Ok(())
    })
}

#[cfg(test)]
thread_local! { static TEST_JOBS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
fn with_jobs<R>(jobs: usize, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_JOBS.with(|s| s.set(self.0));
        }
    }
    let _restore = Restore(TEST_JOBS.with(|s| s.replace(Some(jobs))));
    f()
}
#[cfg(test)]
mod tests;
