// Revoking the shared fact must preserve unpatched method bodies too.
Reflect.set(Array.prototype, "extraPristineProbe", 1);
const a = [1, 2, 3];
console.log(a.push(4), a.map(x => x + 1).join(","), a.indexOf(3));
console.log(a.keys().next().value, a.entries().next().value.join(","));
console.log(a.toReversed().join(","), a.toSorted((x, y) => y - x).join(","));
console.log(a.toSpliced(1, 2, 9).join(","), a.with(1, 8).join(","));
console.log(a.flatMap(x => [x, x]).join(","));
