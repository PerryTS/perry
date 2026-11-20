// Test basic async/await

async function getValue(): Promise<number> {
    return 42;
}

async function run(): Promise<void> {
    let x = await getValue();
    console.log(x); // Should print 42
}

await run();
console.log(100); // After async completes
