// Pattern: many small async chains awaited concurrently. Models
// per-request fan-out (DB + cache + auth + headers in parallel),
// batch processing, request-fan-in patterns.

async function unitOfWork(i: number): Promise<number> {
    // Three-deep await chain, no real I/O — just microtask cost.
    const a = await Promise.resolve(i);
    const b = await Promise.resolve(a + 1);
    const c = await Promise.resolve(b * 2);
    return c;
}

async function main() {
    const N_BATCHES = 1000;
    const BATCH_SIZE = 50;

    let total = 0;
    for (let batch = 0; batch < N_BATCHES; batch++) {
        const promises: Promise<number>[] = [];
        for (let i = 0; i < BATCH_SIZE; i++) {
            promises.push(unitOfWork(batch * BATCH_SIZE + i));
        }
        const results = await Promise.all(promises);
        for (let i = 0; i < results.length; i++) total += results[i];
    }
    console.log("checksum:", total);
}

main();
