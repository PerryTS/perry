// Test returning closures from functions
// Using type inference instead of explicit closure return types

// Test 1: makeCounter - closure that increments and returns a captured counter
function makeCounter() {
    let count = 0;
    return () => {
        count = count + 1;
        return count;
    };
}

let counter1 = makeCounter();
console.log(counter1()); // Should print 1
console.log(counter1()); // Should print 2
console.log(counter1()); // Should print 3

// Create a second independent counter
let counter2 = makeCounter();
console.log(counter2()); // Should print 1
console.log(counter2()); // Should print 2

// First counter should still be at 3, incrementing to 4
console.log(counter1()); // Should print 4

// Test 2: makeAdder - factory function that returns a closure adding to captured value
function makeAdder(x: number) {
    return (y: number) => {
        return x + y;
    };
}

let add5 = makeAdder(5);
let add10 = makeAdder(10);

console.log(add5(3));  // Should print 8
console.log(add10(3)); // Should print 13
console.log(add5(7));  // Should print 12

// Test 3: Closure that reads captured value that was modified before return
function makeAccumulator() {
    let total = 0;
    total = total + 100;
    return () => {
        return total;
    };
}

let acc = makeAccumulator();
console.log(acc()); // Should print 100

console.log("Returning closures test passed!");
