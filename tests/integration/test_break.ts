// Test break statement
let sum = 0;
for (let i = 0; i < 100; i = i + 1) {
    if (i >= 5) {
        break;
    }
    sum = sum + i;
}
console.log(sum); // Should print 10 (0+1+2+3+4)
