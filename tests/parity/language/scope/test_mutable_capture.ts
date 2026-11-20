// Test mutable captured variables
// This should print 1, 2, 3 (not 1, 1, 1)

function counter(): () => number {
    let count = 0;
    return () => {
        count = count + 1;
        return count;
    };
}

let c = counter();
console.log(c()); // Should print 1
console.log(c()); // Should print 2
console.log(c()); // Should print 3
