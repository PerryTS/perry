// Issue #256: spec-compliant microtask ordering for async/await.
// Expected (Node): main-1 / inner-1 / top-1 / top-2 / inner-2 / main-2
// Pre-fix Perry: main-1 / inner-1 / inner-2 / main-2 / top-1 / top-2

async function inner() {
    console.log("inner-1");
    await Promise.resolve();
    console.log("inner-2");
}

async function main() {
    console.log("main-1");
    await inner();
    console.log("main-2");
}

main();
console.log("top-1");
console.log("top-2");
