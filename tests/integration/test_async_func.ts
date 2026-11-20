// Test async function that returns a Promise
async function getNumber(): Promise<number> {
    return 42;
}

// Call the async function and await it
const result = await getNumber();
console.log(result); // Should print 42
