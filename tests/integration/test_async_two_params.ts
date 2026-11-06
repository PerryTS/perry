// Test two locals + param in async function
async function double(x: number): Promise<number> {
    return x * 2;
}

async function addTen(x: number): Promise<number> {
    return x + 10;
}

async function compute(x: number): Promise<number> {
    const d = await double(x);
    const a = await addTen(d);
    return a;
}

const result = await compute(5);
console.log(result); // Should print 20
