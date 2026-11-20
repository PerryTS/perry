// Issue #633 followup (also #611 follow-up): `.then()` on a let-bound
// async-arrow's call result silently dropped its callback because perry's
// is_promise_expr only recognized FuncRef calls (top-level async fns) as
// Promise-returning. A LET-bound `const fn = async (...) => ...; fn(...)`
// call wasn't recognized → the `.then(cb)` lowering at lower_call.rs:1188
// fell through to generic dispatch and the callback never fired.
const fn = async (ctx: any) => { return "X"; };
fn({}).then((r: any) => console.log("then:", r));
