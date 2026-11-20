// Test simpler async/await without async main

async function getValue(): Promise<number> {
    return 42;
}

// Call async function synchronously (fire and forget)
getValue();
console.log(100); // Should print 100
