// #2872: ES2024 Set composition methods (TC39 Set-methods proposal, Node 22+).
const a = new Set<number>([1, 2]);
const b = new Set<number>([2, 3]);
console.log([...a.union(b)]);
console.log([...a.intersection(b)]);
console.log([...a.difference(b)]);
console.log([...a.symmetricDifference(b)]);
console.log(a.isSubsetOf(new Set<number>([1, 2, 3])));
console.log(a.isSupersetOf(new Set<number>([1])));
console.log(a.isDisjointFrom(new Set<number>([4])));

const s1 = new Set<string>(["a", "b", "c"]);
const s2 = new Set<string>(["b", "c", "d"]);
console.log([...s1.union(s2)]);
console.log([...s1.intersection(s2)]);
console.log([...s1.difference(s2)]);
console.log([...s1.symmetricDifference(s2)]);
console.log(s1.isSubsetOf(s2));
console.log(s1.isSupersetOf(new Set<string>(["a"])));
console.log(s1.isDisjointFrom(new Set<string>(["x", "y"])));
