// The array methods a typed array shares (join, sort, map, filter, reverse, forEach,
// reduce, some, every, find, findIndex, ...) run on it in a plain body, each with the
// typed array's own semantics: map and filter make a typed array, sort is numeric.
// On `--target web` the array bridges once answered only arrays, so a typed array
// joined to "", never sorted, and mapped to an empty Array.

const s = (v: any): string => (v && typeof v === "object" ? Object.prototype.toString.call(v).slice(8, -1) + "[" + Array.from(v as ArrayLike<number>).join(",") + "]" : String(v));
const mk = () => new Int16Array([3, 1, 2]);
const out: string[] = [];
const t = (name: string, f: () => any) => { try { out.push(name + "=" + s(f())); } catch (e) { out.push(name + "!" + (e as Error).name); } };
t("join", () => mk().join("-"));
t("join0", () => mk().join());
t("toString", () => mk().toString());
t("slice", () => mk().slice(1));
t("slice2", () => mk().slice(0, 2));
t("subarray", () => mk().subarray(1));
t("sort", () => mk().sort());
t("sortfn", () => mk().sort((a: number, b: number) => b - a));
t("map", () => mk().map((x: number) => x * 2));
t("filter", () => mk().filter((x: number) => x > 1));
t("fill", () => mk().fill(7));
t("fill2", () => mk().fill(7, 1, 2));
t("reverse", () => mk().reverse());
t("forEach", () => { let a = 0; mk().forEach((x: number) => { a += x; }); return a; });
t("reduce", () => mk().reduce((a: number, x: number) => a + x, 0));
t("reduceRight", () => mk().reduceRight((a: number, x: number) => a + "" + x, ""));
t("some", () => mk().some((x: number) => x > 2));
t("every", () => mk().every((x: number) => x > 0));
t("find", () => mk().find((x: number) => x < 3));
t("findIndex", () => mk().findIndex((x: number) => x === 2));
t("findLast", () => mk().findLast((x: number) => x > 1));
t("findLastIndex", () => mk().findLastIndex((x: number) => x > 1));
t("at", () => mk().at(-1));
t("set", () => { const a = mk(); a.set([9, 8], 1); return a; });
t("copyWithin", () => mk().copyWithin(0, 1));
t("entries", () => Array.from(mk().entries()).join("|"));
t("keys", () => Array.from(mk().keys()).join("|"));
t("values", () => Array.from(mk().values()).join("|"));
t("toReversed", () => mk().toReversed());
t("toSorted", () => mk().toSorted());
t("with", () => mk().with(0, 5));
t("from", () => Int16Array.from([1, 2]));
t("of", () => Float64Array.of(1, 2));
t("spread", () => [...mk()].join(","));
t("ArrayFrom", () => Array.from(mk()).join(","));
console.log(out.join("\n"));
