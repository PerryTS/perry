// Test Math functions

// Test Math.floor
console.log(Math.floor(4.7));    // 4
console.log(Math.floor(-4.7));   // -5

// Test Math.ceil
console.log(Math.ceil(4.1));     // 5
console.log(Math.ceil(-4.1));    // -4

// Test Math.round
console.log(Math.round(4.4));    // 4
console.log(Math.round(4.5));    // 5 (or 4 depending on IEEE rounding)

// Test Math.abs
console.log(Math.abs(-5));       // 5
console.log(Math.abs(5));        // 5

// Test Math.sqrt
console.log(Math.sqrt(16));      // 4
console.log(Math.sqrt(2));       // ~1.414

// Test Math.pow
console.log(Math.pow(2, 3));     // 8
console.log(Math.pow(2, 0.5));   // ~1.414

// Test Math.min
console.log(Math.min(1, 2, 3));  // 1
console.log(Math.min(-5, 0, 5)); // -5

// Test Math.max
console.log(Math.max(1, 2, 3));  // 3
console.log(Math.max(-5, 0, 5)); // 5

// Test Math.random
let r = Math.random();
console.log(r);  // Should be between 0 and 1

console.log("Math test passed!");
