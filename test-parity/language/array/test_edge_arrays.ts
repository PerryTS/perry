// Edge-case tests for arrays: methods, indexing, callbacks, mutation, nested arrays
// Targets bugs like: arr[i] returning arr[0], .shift()?.trim() side effects,
// .filter(Boolean), large array init, array methods with closures

// --- Basic array operations ---
const nums = [10, 20, 30, 40, 50];
console.log(nums[0]);           // 10
console.log(nums[4]);           // 50
console.log(nums.length);       // 5

// --- Array iteration with index ---
for (let i = 0; i < nums.length; i++) {
    if (i === 2) console.log(nums[i]);  // 30
}

// --- push / pop ---
const stack: number[] = [];
stack.push(1);
stack.push(2);
stack.push(3);
console.log(stack.length);  // 3
const popped = stack.pop();
console.log(popped);         // 3
console.log(stack.length);   // 2

// --- shift / unshift ---
const queue: number[] = [10, 20, 30];
const shifted = queue.shift();
console.log(shifted);         // 10
console.log(queue.length);    // 2
console.log(queue[0]);        // 20

// --- splice ---
const spliceArr = [1, 2, 3, 4, 5];
const removed = spliceArr.splice(1, 2);
console.log(removed.join(","));     // 2,3
console.log(spliceArr.join(","));   // 1,4,5

// --- slice ---
const sliceArr = [10, 20, 30, 40, 50];
const sliced = sliceArr.slice(1, 3);
console.log(sliced.join(","));     // 20,30
console.log(sliceArr.length);      // 5 (unchanged)

// --- map ---
const doubled = [1, 2, 3].map((x: number) => x * 2);
console.log(doubled.join(","));  // 2,4,6

// --- filter ---
const evens = [1, 2, 3, 4, 5, 6].filter((x: number) => x % 2 === 0);
console.log(evens.join(","));  // 2,4,6

// --- filter(Boolean) - common pattern that caused bugs ---
const mixed: (number | null | string | undefined)[] = [0, 1, null, "hello", undefined, "", 3];
const truthy = mixed.filter(Boolean);
console.log(truthy.length);  // 3 (1, "hello", 3)

// --- reduce ---
const sum = [1, 2, 3, 4, 5].reduce((acc: number, x: number) => acc + x, 0);
console.log(sum);  // 15

// --- find ---
const found = [10, 20, 30, 40].find((x: number) => x > 25);
console.log(found);  // 30

// --- findIndex ---
const idx = [10, 20, 30, 40].findIndex((x: number) => x > 25);
console.log(idx);  // 2

// --- some / every ---
console.log([1, 2, 3].some((x: number) => x > 2));   // true
console.log([1, 2, 3].some((x: number) => x > 5));   // false
console.log([1, 2, 3].every((x: number) => x > 0));  // true
console.log([1, 2, 3].every((x: number) => x > 2));  // false

// --- includes ---
console.log([1, 2, 3].includes(2));  // true
console.log([1, 2, 3].includes(5));  // false

// --- indexOf ---
console.log([10, 20, 30, 20].indexOf(20));  // 1
console.log([10, 20, 30].indexOf(99));       // -1

// --- concat ---
const c1 = [1, 2];
const c2 = [3, 4];
const c3 = c1.concat(c2);
console.log(c3.join(","));  // 1,2,3,4

// --- flat ---
const nested = [[1, 2], [3, 4], [5]];
const flatArr = nested.flat();
console.log(flatArr.join(","));  // 1,2,3,4,5

// --- flatMap ---
const sentences = ["hello world", "foo bar"];
const words = sentences.flatMap((s: string) => s.split(" "));
console.log(words.join(","));  // hello,world,foo,bar

// --- reverse ---
const rev = [1, 2, 3, 4, 5];
rev.reverse();
console.log(rev.join(","));  // 5,4,3,2,1

// --- sort (numeric) ---
const unsorted = [30, 10, 50, 20, 40];
unsorted.sort((a: number, b: number) => a - b);
console.log(unsorted.join(","));  // 10,20,30,40,50

// --- sort (string) ---
const strs = ["banana", "apple", "cherry"];
strs.sort();
console.log(strs.join(","));  // apple,banana,cherry

// --- Array of objects ---
const people = [
    { name: "Alice", age: 30 },
    { name: "Bob", age: 25 },
    { name: "Charlie", age: 35 }
];
const names = people.map((p: { name: string; age: number }) => p.name);
console.log(names.join(","));  // Alice,Bob,Charlie

const older = people.filter((p: { name: string; age: number }) => p.age > 28);
console.log(older.length);  // 2

// --- Nested array access ---
const matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
console.log(matrix[0][0]);  // 1
console.log(matrix[1][1]);  // 5
console.log(matrix[2][2]);  // 9

// --- Array destructuring ---
const [first, second, ...rest] = [10, 20, 30, 40, 50];
console.log(first);           // 10
console.log(second);          // 20
console.log(rest.join(","));  // 30,40,50

// --- Spread operator ---
const arr1 = [1, 2, 3];
const arr2 = [0, ...arr1, 4];
console.log(arr2.join(","));  // 0,1,2,3,4

// --- Array.from with map ---
const mapped = Array.from([1, 2, 3], (x: number) => x * 10);
console.log(mapped.join(","));  // 10,20,30

// --- for...of loop ---
const forOfResult: number[] = [];
for (const val of [100, 200, 300]) {
    forOfResult.push(val);
}
console.log(forOfResult.join(","));  // 100,200,300

// --- Array in conditional ---
const maybeArr: number[] | null = [1, 2, 3];
if (maybeArr) {
    console.log(maybeArr.length);  // 3
}

// --- Chained array methods ---
const chainResult = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    .filter((x: number) => x % 2 === 0)
    .map((x: number) => x * x)
    .reduce((acc: number, x: number) => acc + x, 0);
console.log(chainResult);  // 220

// --- Array length modification ---
const truncArr = [1, 2, 3, 4, 5];
console.log(truncArr.length);  // 5

// --- join with custom separator ---
console.log([1, 2, 3].join(" - "));  // 1 - 2 - 3
console.log([1, 2, 3].join(""));     // 123

// --- Empty array edge cases ---
const emptyArr: number[] = [];
console.log(emptyArr.length);       // 0
console.log(emptyArr.join(","));    // (empty string)
const mapEmpty = emptyArr.map((x: number) => x * 2);
console.log(mapEmpty.length);       // 0

// --- Array with loop variable indexing (regression: arr[i] returned arr[0]) ---
const indexTest = [100, 200, 300, 400, 500];
const collected: number[] = [];
for (let i = 0; i < indexTest.length; i++) {
    collected.push(indexTest[i]);
}
console.log(collected.join(","));  // 100,200,300,400,500

// --- Nested loop with array indexing ---
const grid = [[1, 2], [3, 4], [5, 6]];
let gridSum = 0;
for (let i = 0; i < grid.length; i++) {
    for (let j = 0; j < grid[i].length; j++) {
        gridSum = gridSum + grid[i][j];
    }
}
console.log(gridSum);  // 21

// --- Array.isArray ---
console.log(Array.isArray([1, 2, 3]));  // true
console.log(Array.isArray("hello"));    // false
console.log(Array.isArray(42));         // false

// --- fill ---
const filled = [0, 0, 0, 0, 0];
filled.fill(7);
console.log(filled.join(","));  // 7,7,7,7,7
