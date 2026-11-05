// Test array spread operator

let arr1: number[] = [1, 2, 3];
let arr2: number[] = [4, 5, 6];

// Spread at the end
let combined1: number[] = [...arr1, 7, 8];
console.log(combined1.length);  // 5
console.log(combined1[0]);  // 1
console.log(combined1[3]);  // 7
console.log(combined1[4]);  // 8

// Spread at the beginning
let combined2: number[] = [0, ...arr1];
console.log(combined2.length);  // 4
console.log(combined2[0]);  // 0
console.log(combined2[1]);  // 1

// Multiple spreads
let combined3: number[] = [...arr1, ...arr2];
console.log(combined3.length);  // 6
console.log(combined3[0]);  // 1
console.log(combined3[3]);  // 4
console.log(combined3[5]);  // 6

// Spread with elements in between
let combined4: number[] = [0, ...arr1, 100, ...arr2, 200];
console.log(combined4.length);  // 9
console.log(combined4[0]);  // 0
console.log(combined4[1]);  // 1
console.log(combined4[4]);  // 100
console.log(combined4[8]);  // 200

// Copy array using spread
let copy: number[] = [...arr1];
console.log(copy.length);  // 3
console.log(copy[0]);  // 1
console.log(copy[2]);  // 3

console.log("Spread operator tests passed!");
