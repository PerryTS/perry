async function inner(): Promise<number> { throw new Error("boom"); }

async function main() {
    console.log("before");
    try { await inner(); console.log("after await"); }
    catch (e) { console.log("caught:", (e as any).message); }
    console.log("after try");
    console.log("done");
}
main();
