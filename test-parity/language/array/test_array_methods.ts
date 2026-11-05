// Test array methods: push, pop, shift, unshift, indexOf, includes

let arr: number[] = [1, 2, 3];

// Test push
arr.push(4);
console.log(arr.length); // Should print 4

// Test pop
let popped = arr.pop();
console.log(popped); // Should print 4
console.log(arr.length); // Should print 3

// Test unshift (add to beginning)
arr.unshift(0);
console.log(arr.length); // Should print 4
console.log(arr[0]); // Should print 0

// Test shift (remove from beginning)
let shifted = arr.shift();
console.log(shifted); // Should print 0
console.log(arr.length); // Should print 3
console.log(arr[0]); // Should print 1

// Test indexOf
let idx = arr.indexOf(2);
console.log(idx); // Should print 1

let notFound = arr.indexOf(99);
console.log(notFound); // Should print -1

// Test includes
let has2 = arr.includes(2);
console.log(has2); // Should print 1 (true as number)

let has99 = arr.includes(99);
console.log(has99); // Should print 0 (false as number)

console.log("All array methods tests passed!");
