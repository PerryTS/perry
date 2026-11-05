// Module-level array splice (regression guard)
let items: number[] = [1, 2, 3, 4, 5];
const removed = items.splice(2, 1, 30);
console.log(removed.length);
console.log(removed[0]);
console.log(items.length);
console.log(items[0]);
console.log(items[1]);
console.log(items[2]);
console.log(items[3]);
console.log(items[4]);
