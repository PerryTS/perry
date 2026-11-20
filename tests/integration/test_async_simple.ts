// Simpler async test - one function, one await
async function getValue(): Promise<number> {
    return 100;
}

async function runAsync(): Promise<number> {
    const x = await getValue();
    return x;
}

const result = await runAsync();
console.log(result); // Should print 100
