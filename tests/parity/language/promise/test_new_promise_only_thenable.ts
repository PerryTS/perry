// The one-line program of the report: a promise resolved with a thenable and
// nothing else asynchronous. It printed nothing: `main` ended without the event
// loop (see test_new_promise_only_program.ts), so the thenable job never ran.
//
// Check mode (PERRY_VERIFY_FACTS=1) reported this program until the solve
// modelled the executor's `resolve` adopting a thenable: the `then` body below
// ran while the activation reach called it inactive (the same finding as
// test_gap_promise_tick_parity.ts; `PERRY_FACTS_REACH_THENABLE=0` restores it).
new Promise((res: any) => res({ then(r: any) { r(1); } })).then((v: any) => console.log(v));
