// #7497: `js_get_global_this_builtin_value` read the `globalThis` object out of
// its (correctly registered, correctly rewritten) root into a bare
// `*const ObjectHeader`, and only THEN allocated the lookup key:
//
//     let global_obj = js_nanbox_get_pointer(js_get_global_this());
//     let key = js_string_from_bytes(name);      // ALLOCATES — may collect
//     js_object_get_field_by_name(global_obj, key);   // from-space deref
//
// The root is not the problem; the ORDER is. This lookup interns nothing, so
// every call mints a fresh key string, and any of those allocations can trigger
// the copying minor that evacuates `globalThis`. `THREAD_GLOBAL_THIS` is
// rewritten to the forwarding address — the raw copy already read out of it is
// not.
//
// WHY `Promise.all` IS THE SHAPE THAT FINDS IT. Each element of a combinator
// runs `Call(promiseResolve, C, «next»)`, i.e. `Promise.resolve`, and
// `js_promise_resolve_spec` asks `is_default_promise_constructor` whether `this`
// is the intrinsic `Promise` — which is a `globalThis.Promise` read through the
// function above. One `Promise.all` over 50 000 already-resolved promises
// therefore performs 50 000 of these lookups inside a single native call, so one
// of them straddles the collection. Nothing else in the program has to be large.
//
// STRUCTURE IS LOAD-BEARING. The single big `Promise.all` is what packs enough
// lookups between two collections; the same total work split into many small
// `Promise.all` calls (`benchmarks/app-patterns/kernels/promise_all_chains.ts`
// is 1000 × 50) needs ~20× the work to hit the same window. Do not "simplify"
// this to a loop of small combinator calls.
//
// SYMPTOM BEFORE THE FIX (release, default link, default env): the stale read
// returns whatever from-space holds, so `globalThis.Promise` comes back as a
// non-callable, and the run dies with `Uncaught (in promise) TypeError: value is
// not a function` or, when the garbage happens to be zero, with the resolution
// value arriving on the rejection path: `Uncaught (in promise) 0`.
// `PERRY_GC_PROTECT_FROMSPACE=1` turns it into a precise SIGBUS inside
// `js_object_get_field_by_name`, reached from `js_get_global_this_builtin_value`
// ← `js_promise_resolve_spec` ← `spec_combinators::call_with_this`.

async function main() {
    // One combinator call, wide enough that its per-element
    // `Promise.resolve` lookups span a copying minor.
    const wide: Promise<number>[] = [];
    for (let i = 0; i < 50_000; i++) {
        wide.push(Promise.resolve(i));
    }
    const settled = await Promise.all(wide);

    let checksum = 0;
    let wrong = 0;
    for (let i = 0; i < settled.length; i++) {
        if (settled[i] !== i) wrong++;
        checksum += settled[i];
    }

    // The `globalThis.Promise` identity the stale read corrupts is observable
    // directly too: `Promise.resolve` must keep returning real promises, and a
    // native promise passed to `Promise.resolve` must come back unchanged
    // (`PromiseResolve` short-circuits when `x.constructor === C`).
    const one = Promise.resolve(7);
    const same = Promise.resolve(one) === one;
    const isPromise = one instanceof Promise;

    // The app-pattern shape from the issue: many small combinator calls over
    // three-deep await chains. Smaller here (the wide case above is the
    // sensitive one) but it is the workload #7497 was filed against.
    async function unitOfWork(i: number): Promise<number> {
        const a = await Promise.resolve(i);
        const b = await Promise.resolve(a + 1);
        return b * 2;
    }
    let chained = 0;
    for (let batch = 0; batch < 40; batch++) {
        const promises: Promise<number>[] = [];
        for (let i = 0; i < 50; i++) {
            promises.push(unitOfWork(batch * 50 + i));
        }
        const results = await Promise.all(promises);
        for (let i = 0; i < results.length; i++) chained += results[i];
    }

    console.log("checksum:", checksum, "wrong:", wrong);
    console.log("same:", same, "isPromise:", isPromise);
    console.log("chained:", chained);
}

main();
