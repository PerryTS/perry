// Test generic functions
// Phase 3 test: verify type parameters are extracted

// Test simple generic function with numbers only
function identity<T>(x: T): T {
    return x;
}

// Test with explicit type arguments (numbers only to avoid type mismatch)
let n: number = identity<number>(42);
console.log(n);  // Should print 42

// Test with another number
let m: number = identity<number>(100);
console.log(m);  // Should print 100

// Test multiple type parameters (both numbers)
function pair<T, U>(a: T, b: U): number {
    return 0;
}

let p = pair<number, number>(1, 2);
console.log(p);  // Should print 0

console.log("Generic function tests passed!");
