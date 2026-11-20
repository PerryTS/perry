// Test array destructuring

let arr: number[] = [1, 2, 3, 4, 5];

// Basic array destructuring
let [a, b, c] = arr;
console.log(a);  // 1
console.log(b);  // 2
console.log(c);  // 3

// Destructuring with fewer variables than elements
let [first, second] = arr;
console.log(first);   // 1
console.log(second);  // 2

// Nested destructuring test - extract from function return
function getCoords(): number[] {
    return [10, 20];
}

let [x, y] = getCoords();
console.log(x);  // 10
console.log(y);  // 20

// Destructuring in a loop (manual for now since for-of destructuring is complex)
let points: number[] = [100, 200, 300];
let [p1, p2, p3] = points;
console.log(p1);  // 100
console.log(p2);  // 200
console.log(p3);  // 300

console.log("Destructuring tests passed!");
