// Correctness: empty, 1-elem, many-elem, mixed types, nested, side-effecting elements
const empty: any[] = [];
console.log("empty.length:", empty.length);

const one = [42];
console.log("one:", one[0], one.length);

const three = [1, 2, 3];
console.log("three:", three[0], three[1], three[2], three.length);

const eight = [10, 20, 30, 40, 50, 60, 70, 80];
console.log("eight sum:", eight[0] + eight[1] + eight[2] + eight[3] + eight[4] + eight[5] + eight[6] + eight[7]);

// N=16 should still work (exact-sized path, not MIN_ARRAY_CAPACITY padded).
const sixteen = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
console.log("sixteen last:", sixteen[15], "len:", sixteen.length);

// Heterogeneous: string + number + boolean mixed.
const mixed: any[] = [1, "two", true, null];
console.log("mixed:", mixed[0], mixed[1], mixed[2], mixed[3]);

// Nested — inner array literal allocated inside element evaluation.
const nested = [[1, 2], [3, 4], [5, 6]];
console.log("nested[1][0]:", nested[1][0], "nested[2][1]:", nested[2][1]);

// Side-effecting element expressions.
let counter = 0;
function bump(): number { counter += 1; return counter; }
const fx = [bump(), bump(), bump()];
console.log("fx:", fx[0], fx[1], fx[2], "counter:", counter);

// Array from a literal then pushing (validates capacity growth still works).
const grow: number[] = [1, 2, 3];
for (let i = 0; i < 20; i++) grow.push(i);
console.log("grow.length:", grow.length, "grow[0]:", grow[0], "grow[22]:", grow[22]);

// String concat inside literal (exercises the "element allocates before array alloc" path).
const strs = ["a" + "b", "c" + "d", "e" + "f"];
console.log("strs:", strs[0], strs[1], strs[2]);
