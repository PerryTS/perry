// Test increment/decrement operators

// Basic post-increment
let x = 5;
x++;
console.log(x); // Should be 6

// Basic post-decrement
let y = 10;
y--;
console.log(y); // Should be 9

// Pre-increment
let a = 3;
console.log(++a); // Should be 4
console.log(a);   // Should be 4

// Pre-decrement
let b = 7;
console.log(--b); // Should be 6
console.log(b);   // Should be 6

// Post-increment in expression
let c = 5;
console.log(c++); // Should be 5 (returns old value)
console.log(c);   // Should be 6

// Post-decrement in expression
let d = 5;
console.log(d--); // Should be 5 (returns old value)
console.log(d);   // Should be 4

// In for loop
let sum = 0;
for (let i = 0; i < 5; i++) {
    sum = sum + i;
}
console.log(sum); // Should be 10 (0+1+2+3+4)
