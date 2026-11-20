// Test array splice method
let arr: number[] = [1, 2, 3, 4, 5];

// Test 1: splice with delete only (remove 2 elements starting at index 1)
let deleted = arr.splice(1, 2);
console.log(deleted.length);  // 2
console.log(deleted[0]);      // 2
console.log(deleted[1]);      // 3
console.log(arr.length);      // 3
console.log(arr[0]);          // 1
console.log(arr[1]);          // 4
console.log(arr[2]);          // 5

// Test 2: splice with insert (add elements at index 1, delete 0)
arr = [1, 2, 3];
deleted = arr.splice(1, 0, 10, 20);
console.log(deleted.length);  // 0 (nothing deleted)
console.log(arr.length);      // 5
console.log(arr[0]);          // 1
console.log(arr[1]);          // 10
console.log(arr[2]);          // 20
console.log(arr[3]);          // 2
console.log(arr[4]);          // 3

// Test 3: splice with delete and insert (replace)
arr = [1, 2, 3, 4, 5];
deleted = arr.splice(2, 2, 100, 200, 300);
console.log(deleted.length);  // 2
console.log(deleted[0]);      // 3
console.log(deleted[1]);      // 4
console.log(arr.length);      // 6
console.log(arr[0]);          // 1
console.log(arr[1]);          // 2
console.log(arr[2]);          // 100
console.log(arr[3]);          // 200
console.log(arr[4]);          // 300
console.log(arr[5]);          // 5

// Test 4: splice with negative index
arr = [1, 2, 3, 4, 5];
deleted = arr.splice(-2, 1);  // Start from 2nd last
console.log(deleted.length);  // 1
console.log(deleted[0]);      // 4
console.log(arr.length);      // 4
console.log(arr[3]);          // 5

console.log("Splice tests passed!");
