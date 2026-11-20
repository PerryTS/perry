// Test async with pending promise (tests the busy-wait loop)
// Promise.delayed creates a pending promise that resolves on next microtask

// This creates a promise that starts PENDING and gets resolved when microtasks run
const promise = Promise.delayed(42);

// The await here should spin in the busy-wait loop until the promise resolves
const result = await promise;

console.log(result); // Should print 42
