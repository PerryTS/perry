// Correctness test for parseFloat — covers edge cases that match Node's behaviour.
// Diff-tested byte-for-byte against node --experimental-strip-types.

// Well-formed inputs
console.log(parseFloat("3.14"));               // 3.14
console.log(parseFloat("1e10"));               // 10000000000
console.log(parseFloat("-0.5"));               // -0.5
console.log(parseFloat("1234567890.12345"));   // 1234567890.12345
console.log(parseFloat("0"));                  // 0
console.log(parseFloat("42"));                 // 42
console.log(parseFloat(".5"));                 // 0.5
console.log(parseFloat("5."));                 // 5
console.log(parseFloat("+3.14"));              // 3.14

// Leading whitespace (JS strips it)
console.log(parseFloat("  3.14"));             // 3.14
console.log(parseFloat("\t3.14"));             // 3.14
console.log(parseFloat("\n3.14"));             // 3.14

// Trailing junk — parseFloat stops at first invalid char
console.log(parseFloat("3.14abc"));            // 3.14
console.log(parseFloat("1e10xyz"));            // 10000000000
console.log(parseFloat("42 extra"));           // 42
console.log(parseFloat("1e"));                 // 1
console.log(parseFloat("1e+"));                // 1

// Invalid inputs
console.log(parseFloat("abc"));               // NaN
console.log(parseFloat(""));                  // NaN
console.log(parseFloat("   "));               // NaN
console.log(parseFloat("."));                 // NaN
console.log(parseFloat("+"));                 // NaN
console.log(parseFloat("-"));                 // NaN

// Infinity variants
console.log(parseFloat("Infinity"));          // Infinity
console.log(parseFloat("-Infinity"));         // -Infinity
console.log(parseFloat("+Infinity"));         // Infinity
console.log(parseFloat("  Infinity"));        // Infinity
