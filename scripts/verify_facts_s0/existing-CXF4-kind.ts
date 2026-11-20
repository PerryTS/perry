const P: any = { kind: "proto", base: 10, describe() { return this.kind; } };
const o = Object.assign(Object.create(P), { value: 7, label: "n", kind: "own" });
console.log(o.kind, P.kind);
