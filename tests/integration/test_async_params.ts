// Test async functions with parameters
async function double(x: number): Promise<number> {
    return x * 2;
}

async function process(n: number): Promise<number> {
    const d = await double(n);
    return d;
}

const result = await process(5);
console.log(result); // Should print 10
