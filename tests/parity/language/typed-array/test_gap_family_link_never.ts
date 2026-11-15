// family_reach, the byte-cell family is "never": this program creates no
// typed array, no Buffer and no ArrayBuffer and names none of them, so its
// typed-array runtime group is not linked. Everything it does must still be
// right: the comparator sort's private index cells (more than 64 elements)
// are no values, and receivers that merely look like typed arrays are
// ordinary objects.
const xs: number[] = [];
for (let i = 0; i < 300; i++) xs.push((i * 7919) % 307);
xs.sort((a, b) => a - b);
console.log("sort", xs[0], xs[1], xs[149], xs[299], xs.length);
const desc = xs.slice().sort((a, b) => b - a);
console.log("desc", desc[0], desc[299]);
const words = Array.from({ length: 120 }, (_, i) => "w" + ((i * 37) % 120));
words.sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
console.log("words", words[0], words[60], words[119]);

// Objects with the method names of the typed-array tower: their own methods run.
const lookalike: any = {
  length: 3,
  at(i: number) { return "at:" + i; },
  fill(v: number) { return "fill:" + v; },
  indexOf(v: number) { return "indexOf:" + v; },
  subarray(a: number) { return "subarray:" + a; },
  set(v: number) { return "set:" + v; },
  join(s: string) { return "join:" + s; },
};
function drive(o: any): string[] {
  return [o.at(1), o.fill(2), o.indexOf(3), o.subarray(4), o.set(5), o.join("-")];
}
console.log(JSON.stringify(drive(lookalike)));
try {
  console.log(JSON.stringify(drive([10, 20, 30])));
} catch (e) {
  console.log((e as Error).constructor.name, [10, 20, 30].at(1), [10, 20, 30].fill(2).join());
}

// Array methods of the same names on real Arrays.
const arr = [5, 1, 4];
console.log(arr.at(-1), arr.indexOf(4), arr.includes(1), arr.join("+"), arr.slice(1).length);
console.log([3, 1, 2].toSorted().join(), [3, 1, 2].toReversed().join(), [1, 2, 3].with(1, 9).join());
console.log(Array.from("abc").map((c) => c.charCodeAt(0)).join());
