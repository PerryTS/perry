// Test await inside async function returning a value

async function getValue(): Promise<number> {
    return 42;
}

async function doubleValue(): Promise<number> {
    let x = await getValue();
    return x + x;
}

let result = await doubleValue();
console.log(result); // Should print 84
