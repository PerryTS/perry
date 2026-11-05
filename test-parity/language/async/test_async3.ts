// Test await with regular (non-async) main

async function getValue(): Promise<number> {
    return 42;
}

// This is NOT an async function, just regular top-level code
let p = getValue();
console.log(123); // Print before await

// Await the promise
let result = await p;
console.log(result); // Should print 42
