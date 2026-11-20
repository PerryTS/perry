// Regression test for issue #233.
//
// Pre-fix: Array.push from inside an async function silently capped at
// 16 elements (the default initial capacity) when the array was passed
// in as a function parameter. js_array_push_f64 reallocates the array
// when capacity is exhausted and returns the new pointer, but the
// async caller's parameter slot was never updated with that new
// pointer — so subsequent pushes operated on a defunct OLD ArrayHeader
// while caller's view stayed stuck at length=16.
//
// Fix: js_array_grow installs a forwarding pointer at the OLD location
// (reusing the GC's GC_FLAG_FORWARDED mechanism). clean_arr_ptr +
// js_value_length_f64 + the inline-length codegen fast path all detect
// and follow the forwarding chain, so the caller's stale pointer
// transparently resolves to the live new array.

async function pushAsync(arr: number[], v: number): Promise<void> {
    arr.push(v);
}

async function pushStringAsync(arr: string[], v: string): Promise<void> {
    arr.push(v);
}

async function pushObjectAsync(arr: { id: number }[], v: { id: number }): Promise<void> {
    arr.push(v);
}

async function pushManyAcrossAwaits(arr: number[], n: number): Promise<void> {
    for (let i = 0; i < n; i++) {
        await pushAsync(arr, i);
    }
}

async function main() {
    // Case 1: number[] across 25 iterations through an async wrapper.
    const samples: number[] = [];
    for (let i = 0; i < 25; i++) {
        await pushAsync(samples, i);
    }
    console.log("number[] length:", samples.length);
    console.log("number[] [15]:", samples[15]);
    console.log("number[] [24]:", samples[24]);

    // Case 2: string[] — verify the bug isn't type-specific.
    const strs: string[] = [];
    for (let i = 0; i < 20; i++) {
        await pushStringAsync(strs, "v" + i);
    }
    console.log("string[] length:", strs.length);
    console.log("string[] [16]:", strs[16]);

    // Case 3: object[] — pointer-typed elements via async push.
    const objs: { id: number }[] = [];
    for (let i = 0; i < 20; i++) {
        await pushObjectAsync(objs, { id: i });
    }
    console.log("object[] length:", objs.length);
    console.log("object[] [17].id:", objs[17].id);

    // Case 4: nested async — outer awaits an async helper that itself
    // awaits the inner pusher. Two levels of stale-parameter risk.
    const nested: number[] = [];
    await pushManyAcrossAwaits(nested, 30);
    console.log("nested length:", nested.length);
    console.log("nested [29]:", nested[29]);

    // Case 5: read after push — make sure subsequent operations also
    // follow the forwarding chain (slice, indexOf, iteration).
    const post: number[] = [];
    for (let i = 0; i < 20; i++) {
        await pushAsync(post, i * 10);
    }
    const sliced = post.slice(15, 19);
    console.log("slice length:", sliced.length);
    console.log("slice [0]:", sliced[0]);
    console.log("indexOf 170:", post.indexOf(170));
    let sum = 0;
    for (const v of post) sum += v;
    console.log("sum:", sum);

    // Case 6: IndexSet through a stale pointer. After the async push
    // grew the array, the caller's slot still holds the OLD pointer.
    // `arr[i] = v` from the caller must follow the forwarding chain
    // and write to the LIVE array, otherwise the next read returns
    // the original value.
    const indexed: number[] = [];
    for (let i = 0; i < 20; i++) {
        await pushAsync(indexed, i);
    }
    indexed[17] = 999;
    console.log("indexed [17] after set:", indexed[17]);
    indexed[5] = 555; // within OLD's range, should still write to live
    console.log("indexed [5] after set:", indexed[5]);
}

main();
