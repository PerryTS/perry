// Debug async test
async function getValue(): Promise<number> {
    console.log(1);
    return 100;
}

console.log(0);
const result = await getValue();
console.log(2);
console.log(result);
