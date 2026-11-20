// Issue #619 repro #1: sync fn throws at await position
function syncThrower(): any { throw new Error("sync error"); }
async function main1() {
    console.log("--- repro 1 ---");
    console.log("before try");
    try { await syncThrower(); }
    catch (e) { console.log("caught:", (e as any).message); }
    console.log("done");
}

// Issue #619 repro #2: async fn whose body throws sync
async function inner(): Promise<any> { throw new Error("from inner"); }
async function main2() {
    console.log("--- repro 2 ---");
    try { await inner(); }
    catch (e) { console.log("caught:", (e as any).message); }
}

async function main() {
    await main1();
    await main2();
}
main();
