// Test try-catch-finally

// Basic try-catch
try {
    console.log(1);
    throw 42;
    console.log(2); // Should not print
} catch (e) {
    console.log(e); // Should print 42
}
console.log(3); // Should print 3

// Try-catch with finally
try {
    console.log(10);
} catch (e) {
    console.log(99); // Should not print
} finally {
    console.log(11); // Should always print
}
console.log(12);
