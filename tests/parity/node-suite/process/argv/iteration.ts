// process.argv supports `for...of` iteration.
let count = 0;
for (const _ of process.argv) count++;
console.log("for-of count > 0:", count > 0);
console.log("argv[0] non-empty:", process.argv[0].length > 0);
