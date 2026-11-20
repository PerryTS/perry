// Test continue statement - skip even numbers
let sum = 0;
for (let i = 0; i < 10; i = i + 1) {
    if (i % 2 == 0) {
        continue;
    }
    sum = sum + i;
}
console.log(sum); // Should print 25 (1+3+5+7+9)
