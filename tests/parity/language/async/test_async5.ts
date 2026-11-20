// Test async function that returns void

async function doSomething(): Promise<void> {
    console.log(1);
    let x = await getValue();
    console.log(x);
    console.log(3);
}

async function getValue(): Promise<number> {
    return 42;
}

await doSomething();
console.log(999);
