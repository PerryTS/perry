// Test for-in loops (iterating over object keys)

// Basic for-in loop over object literal
let obj = { a: 1, b: 2, c: 3 };
for (let key in obj) {
    console.log(key);  // Should print "a", "b", "c"
}

// For-in with object containing different value types
let person = { name: 42, age: 25, active: 1 };
let keyCount = 0;
for (let k in person) {
    console.log(k);
    keyCount = keyCount + 1;
}
console.log(keyCount);  // Should print 3

// For-in in a function
function countKeys(o: { x: number, y: number, z: number }): number {
    let count = 0;
    for (let key in o) {
        count = count + 1;
    }
    return count;
}

let point = { x: 10, y: 20, z: 30 };
console.log(countKeys(point));  // Should print 3
