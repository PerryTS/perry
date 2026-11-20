// Test generic functions

function identity<T>(x: T): T {
    return x;
}

// Test with explicit type arguments
let n: number = identity<number>(42);
let s: string = identity<string>("hello");

console.log(n);
console.log(s);
