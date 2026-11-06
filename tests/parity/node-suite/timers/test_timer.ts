// Test setTimeout with async/await

console.log(1);

// Wait for 100ms
await setTimeout(100);

console.log(2);

// Wait for another 50ms
await setTimeout(50);

console.log(3);
