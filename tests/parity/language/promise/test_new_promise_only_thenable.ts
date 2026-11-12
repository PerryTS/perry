// The one-line program of the report: a promise resolved with a thenable and
// nothing else asynchronous. It printed nothing: `main` ended without the event
// loop (see test_new_promise_only_program.ts), so the thenable job never ran.
//
// Check mode (PERRY_VERIFY_FACTS=1) reports this program until the solve
// models the executor's `resolve` adopting a thenable: the `then` body below
// runs while the activation reach calls it inactive (the same finding as
// test_gap_promise_tick_parity.ts).
new Promise((res: any) => res({ then(r: any) { r(1); } })).then((v: any) => console.log(v));
