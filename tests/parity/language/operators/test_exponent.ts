// Test exponentiation operator (**)

// Basic exponentiation
console.log(2 ** 3);     // 8
console.log(3 ** 2);     // 9
console.log(10 ** 0);    // 1
console.log(2 ** 10);    // 1024

// Fractional exponents
console.log(4 ** 0.5);   // 2 (square root)
console.log(8 ** (1/3)); // 2 (cube root)

// Negative exponents
console.log(2 ** -1);    // 0.5
console.log(10 ** -2);   // 0.01

// Chained exponentiation (right-associative)
console.log(2 ** 3 ** 2); // 2 ** 9 = 512

// With variables
let base = 5;
let exp = 3;
console.log(base ** exp); // 125

// Compound assignment **=
let x = 2;
x **= 4;
console.log(x);  // 16

let y = 3;
y **= 2;
console.log(y);  // 9

// Compare with Math.pow
console.log(2 ** 8);         // 256
console.log(Math.pow(2, 8)); // 256

console.log("Exponent test passed!");
