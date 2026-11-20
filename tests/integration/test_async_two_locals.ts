// Test two local variables in async function
async function getA(): Promise<number> {
    return 10;
}

async function getB(): Promise<number> {
    return 20;
}

async function add(): Promise<number> {
    const a = await getA();
    const b = await getB();
    return a + b;
}

const result = await add();
console.log(result); // Should print 30
