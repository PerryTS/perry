// MUST NOT take the direct generator step: the same generator body iterates a
// generator, an array, a Set, a Map, a string and a user iterable.
function* count(n: number): Generator<number> { for (let i = 0; i < n; i++) yield i; }
function* wrap(src: any): Generator<any> { for (const v of src) yield v; }
const user = { [Symbol.iterator]() { let i = 0; return { next: () => (i < 2 ? { value: "u" + i++, done: false } : { value: undefined, done: true }) }; } };
console.log([...wrap(count(3))].join(","));
console.log([...wrap([4, 5])].join(","));
console.log([...wrap(new Set([6, 7]))].join(","));
console.log([...wrap(new Map([[8, 9]]))].map(String).join(","));
console.log([...wrap("ab")].join(","));
console.log([...wrap(user)].join(","));
// A generator object with an own next(): that next is what the loop calls.
const g: any = count(5);
g.next = () => ({ value: "own", done: g.calls++ > 0 });
g.calls = 0;
console.log([...wrap(g)].join(","));
