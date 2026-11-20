// Test async function calling another async function
async function inner(): Promise<number> {
    return 50;
}

async function outer(): Promise<number> {
    const val = await inner();
    return val;
}

const result = await outer();
console.log(result); // Should print 50
