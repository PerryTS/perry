// Test async functions calling each other

async function double(x: number): Promise<number> {
    return x * 2;
}

async function addTen(x: number): Promise<number> {
    return x + 10;
}

async function compute(x: number): Promise<number> {
    const doubled = await double(x);
    const added = await addTen(doubled);
    return added;
}

// Call compute(5) -> double(5)=10 -> addTen(10)=20
const result = await compute(5);
console.log(result); // Should print 20
