// Test try-catch
let result = 0;

try {
    result = 1;
    throw 42;
    result = 2; // Should not reach here
} catch (e) {
    result = 3;
    console.log(e); // Should print the thrown value (42)
}

console.log(result); // Should print 3
