// #10023: array destructuring puts the binding inside a synthetic HIR Try.
// Its live range can extend past that Try and across a later await.
async function rows(id: number) {
    return [{ id, maxRedemptions: id === 2 ? null : 4 }];
}
async function count() { return [{ n: 0 }]; }
async function main() {
    const results: number[] = [];
    const captures: (() => number)[] = [];
    for (const id of [1, 2, 3]) {
        const [row] = await rows(id);
        if (row.maxRedemptions !== null) {
            const [total] = await count();
            if (total.n >= row.maxRedemptions) throw new Error("limit");
        }
        await Promise.resolve();
        results.push(row.id);
        captures.push(() => row.id);
    }
    console.log(JSON.stringify(results));
    console.log(JSON.stringify(captures.map(read => read())));

    // A later default initializer can suspend inside the synthetic Try too.
    const defaults: number[] = [];
    for (const id of [4, 5]) {
        const [row, fallback = await Promise.resolve(id + 10)] = [id];
        await Promise.resolve();
        defaults.push(row + fallback);
    }
    console.log(JSON.stringify(defaults));
}
main().catch(error => { console.error(error); process.exitCode = 1; });
