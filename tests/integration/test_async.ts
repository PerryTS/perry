// Simple async/await test - direct await without async function wrapper
// This is technically invalid JS (await outside async) but tests our codegen

// Create a resolved promise
const promise = Promise.resolve(42);

// Await it directly (our compiler allows this for now)
const result = await promise;

console.log(result); // Should print 42
