// The one-line program of the report: a promise resolved with a thenable,
// nothing else asynchronous. See test_new_promise_only_program.ts.
new Promise((res: any) => res({ then(r: any) { r(1); } })).then((v: any) => console.log(v));
