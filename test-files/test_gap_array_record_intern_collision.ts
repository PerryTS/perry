// Key interning may evict an atom while the prototype shape still owns it.
const originalNext = Object.getPrototypeOf([][Symbol.iterator]()).next;
const keys: any = {};
for (let i = 0; i < 10000; i++) keys["array-record-key-" + i] = i;
const arr: any = [1, 2, 3];
let sum = 0;
for (const value of arr) sum += value;
const [a, b, ...rest] = arr;
console.log(sum, a, b, rest.join(","));
Object.getPrototypeOf(arr[Symbol.iterator]()).next = function () {
    return {done: true, value: undefined};
};
let count = 0;
for (const value of arr) count++;
console.log(count);
Object.getPrototypeOf(arr[Symbol.iterator]()).next = originalNext;
