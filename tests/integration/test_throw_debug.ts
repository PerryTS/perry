// Debug throw
console.log(1);
try {
    console.log(2);
    throw 42;
    console.log(3); // Should not reach
} catch (e) {
    console.log(4);
}
console.log(5);
