// Test array slice method
let arr: number[] = [1, 2, 3, 4, 5];

// Basic slice
let slice1: number[] = arr.slice(1, 3);
console.log(slice1.length);  // 2
console.log(slice1[0]);  // 2
console.log(slice1[1]);  // 3

// Slice from start
let slice2: number[] = arr.slice(2);
console.log(slice2.length);  // 3

// Negative indices
let slice3: number[] = arr.slice(-2);
console.log(slice3.length);  // 2
console.log(slice3[0]);  // 4
console.log(slice3[1]);  // 5

console.log("Array slice tests passed!");
