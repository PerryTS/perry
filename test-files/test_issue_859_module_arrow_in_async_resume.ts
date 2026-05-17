// Issue #859: the async-to-generator transform's `IterResultSet(value, ...)`
// (and the sibling `AsyncStepChain` / `AsyncStepDone` / `AsyncFirstCall`)
// variants were not visited by `collect_ref_ids_in_expr` in
// `crates/perry-codegen/src/collectors.rs` — its `_ => {}` catch-all
// silently dropped child Exprs of variants added by later HIR passes.
//
// The user-visible failure: a module-level `const f = (x) => …` arrow
// called from inside a `was_plain_async` function's resumed step body
// (state-machine state ≥ 1 — i.e. after at least one `await`) compiled
// with the `LocalGet(toUser_id)` callee lowering to a literal `0.0`,
// because the module-globals pre-walk in `compile_module` never saw the
// reference. The runtime then dispatched `js_closure_call1(0, arg)` and
// SIGSEGVed on the null closure pointer.
//
// Repro shape: nested user async function awaits a named async
// function, then calls a module-level const arrow. Function declarations
// take the FuncRef path which has its own dispatch, so they were never
// broken — only arrow values stored in `const` were affected.
//
// Post-fix the catch-all delegates to `walker::walk_expr_children`, so
// any HIR variant the explicit match doesn't list still gets descended
// into. New variants added by future transforms can't reintroduce the
// silent-skip class.

const toUser = (r: unknown): unknown => r;

async function delay(): Promise<void> {
  await Promise.resolve();
}

async function createUser(): Promise<unknown> {
  await delay();
  // The bug site: `LocalGet(toUser)` reached after the resume.
  return toUser(42);
}

async function go(): Promise<{ ok: boolean; u: unknown }> {
  const u = await createUser();
  return { ok: true, u };
}

console.log(JSON.stringify(await go()));
