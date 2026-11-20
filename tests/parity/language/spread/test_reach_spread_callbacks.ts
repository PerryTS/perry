// A spread argument reaches the positions a built-in reads: the callback of
// `xs.forEach(...[cb])`, a `thisArg`, an initial value, an inserted element.
// The whole-program solve's hand-written rules read their positions among
// the direct arguments only, found no callback, and called the callback's
// body inactive while it ran (check mode `PERRY_VERIFY_FACTS=1`: "inactive
// body: body entered"; with `PERRY_FACTS_DCE=1` the removed body trapped).
const xs: number[] = [1, 2, 3];
const none: any[] = [];

let sum = 0;
xs.forEach(...[(x: number) => { sum += x; }]);
console.log("forEach:", sum);

sum = 0;
(xs.forEach as any)(...none, (x: number) => { sum += x * 2; });
console.log("forEach-after-spread:", sum);

const holder = { k: 10, seen: 0 };
(xs.forEach as any)(...[function (this: any, x: number) { this.seen += x * this.k; }, holder]);
console.log("forEach-thisArg:", holder.seen);

console.log("findIndex:", xs.findIndex(...[(x: number) => x === 2] as [any]));
console.log("findLast:", (xs as any).findLast(...[(x: number) => x < 3]));
console.log("findLastIndex:", (xs as any).findLastIndex(...[(x: number) => x < 3]));
console.log("flatMap:", xs.flatMap(...[(x: number) => [x, x * 10]] as [any]).join("|"));
console.log("reduce:", (xs.reduce as any)(...[(a: number, x: number) => a + x, 100]));
console.log("reduceRight:", (xs.reduceRight as any)(...[(a: string, x: number) => a + x, ""]));
console.log("toSorted:", (xs as any).toSorted(...[(a: number, b: number) => b - a]).join("|"));
console.log("map:", xs.map(...[(x: number) => x + 1] as [any]).join("|"));
console.log("filter:", xs.filter(...[(x: number) => x > 1] as [any]).join("|"));
console.log("sort:", [3, 1, 2].sort(...[(a: number, b: number) => a - b] as [any]).join("|"));

// Without an initial value the accumulator starts as the first element.
const fns: Array<() => string> = [() => "first", () => "second"];
console.log("reduce-no-initial:", (fns.reduce as any)(...[(a: () => string) => a])());

// A spread element is the inserted value.
const inserted: Array<() => string> = [];
(inserted.splice as any)(...[0, 0, () => "spliced"]);
console.log("splice:", inserted[0]());

let it = 0;
(xs.values() as any).forEach(...[(x: number) => { it += x; }]);
console.log("iterator-forEach:", it);
console.log("iterator-map:", [...(xs.values() as any).map(...[(x: number) => x * 3])].join("|"));
console.log("iterator-reduce:", (xs.values() as any).reduce(...[(a: number, x: number) => a + x, 1]));

let mv = 0;
(new Map([[1, 20]]).forEach as any)(...[(v: number, k: number) => { mv += v + k; }]);
console.log("map-forEach:", mv);
const setHolder = { n: 0 };
(new Set([5]).forEach as any)(...[function (this: any, v: number) { this.n += v; }, setHolder]);
console.log("set-forEach-thisArg:", setHolder.n);

console.log("array-from:", (Array.from as any)(...[xs, (x: number) => x * 2]).join("|"));
console.log("json-parse:", (JSON.parse as any)(...['{"a":1,"b":2}', (k: string, v: any) => (typeof v === "number" ? v + 1 : v)]).b);
console.log("string-replace:", ("abc".replace as any)(...["b", (s: string) => s.toUpperCase()]));

(Promise.resolve(7).then as any)(...[(v: number) => { console.log("then:", v); }]);
(Promise.reject(8).catch as any)(...[(v: number) => { console.log("catch:", v); }]);
(Promise.resolve(() => "passed").then as any)(...none).then((f: () => string) => console.log("then-empty-spread:", f()));
