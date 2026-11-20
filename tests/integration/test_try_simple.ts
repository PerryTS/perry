// Test simple try without throw
let result = 0;

try {
    result = 1;
    console.log(1);
} catch (e) {
    result = 2;
    console.log(2);
}

console.log(result); // Should print 1
