// Test for loop

// Basic for loop - sum 0 to 9
let sum = 0;
for (let i = 0; i < 10; i = i + 1) {
    sum = sum + i;
}
console.log(sum); // Should print 45

// For loop with different step
let sum2 = 0;
for (let i = 0; i < 20; i = i + 2) {
    sum2 = sum2 + i;
}
console.log(sum2); // Should print 0+2+4+6+8+10+12+14+16+18 = 90

// Nested for loops
let product = 0;
for (let i = 1; i <= 3; i = i + 1) {
    for (let j = 1; j <= 3; j = j + 1) {
        product = product + i * j;
    }
}
console.log(product); // Should print 36 (1*1+1*2+1*3+2*1+2*2+2*3+3*1+3*2+3*3)
