// Test break and continue

// Test break - stop at 5
let sum = 0;
for (let i = 0; i < 10; i = i + 1) {
    if (i === 5) {
        break;
    }
    sum = sum + i;
}
console.log(sum); // Should print 0+1+2+3+4 = 10

// Test continue - skip 5
let sum2 = 0;
for (let i = 0; i < 10; i = i + 1) {
    if (i === 5) {
        continue;
    }
    sum2 = sum2 + i;
}
console.log(sum2); // Should print 0+1+2+3+4+6+7+8+9 = 40

// Test break in while loop
let sum3 = 0;
let j = 0;
while (j < 10) {
    if (j === 5) {
        break;
    }
    sum3 = sum3 + j;
    j = j + 1;
}
console.log(sum3); // Should print 10
