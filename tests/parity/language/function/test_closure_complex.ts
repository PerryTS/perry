// Test various closure scenarios

// Test 1: Mutable capture (counter)
function makeCounter(): () => number {
    let count = 0;
    return () => {
        count = count + 1;
        return count;
    };
}

console.log(42); // Separator
let c1 = makeCounter();
console.log(c1()); // 1
console.log(c1()); // 2
console.log(c1()); // 3

// Test 2: Multiple counters are independent
let c2 = makeCounter();
console.log(c2()); // 1 (independent from c1)
console.log(c1()); // 4 (c1 continues)

// Test 3: Immutable capture (should still work)
function makeAdder(x: number): (y: number) => number {
    return (y: number) => x + y;
}

let add5 = makeAdder(5);
console.log(add5(10)); // 15
console.log(add5(20)); // 25

// Test 4: Mixed mutable and immutable captures
function makeAccumulator(initial: number): () => number {
    let sum = initial;
    return () => {
        sum = sum + 1;
        return sum;
    };
}

let acc = makeAccumulator(100);
console.log(acc()); // 101
console.log(acc()); // 102
