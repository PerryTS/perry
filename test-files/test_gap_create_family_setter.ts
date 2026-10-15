// Object model step 1: a key add on an `Object.create(literal)` object must
// run a setter its literal prototype carries, whether the setter is written
// in the literal or defined on it later, and must not create an own key.

const log: string[] = [];
const WithSetter: any = {
  set value(v: number) { log.push("lit:" + v); },
  get value() { return -1; },
};
const Plain: any = { other: 1 };
Object.defineProperty(Plain, "late", {
  set(v: number) { log.push("late:" + v); },
  get() { return -2; },
  configurable: true,
});

for (let i = 0; i < 3; i++) {
  const a = Object.assign(Object.create(WithSetter), { value: i, keep: i });
  const b = Object.assign(Object.create(Plain), { late: i, keep: i });
  console.log(Object.keys(a).join(","), a.value, Object.keys(b).join(","), b.late);
}
console.log(log.join(" "));

// A setter added to Object.prototype intercepts too.
Object.defineProperty(Object.prototype, "viaObjectProto", {
  set(v: number) { log.push("op:" + v); },
  configurable: true,
});
const c = Object.assign(Object.create(Plain), { viaObjectProto: 5, z: 1 });
console.log(Object.keys(c).join(","), log[log.length - 1]);
delete (Object.prototype as any).viaObjectProto;

// The same through a plain key add on `const x = Object.create(P)`.
const s: any = Object.create(WithSetter);
s.value = 7;
const t: any = Object.create(Plain);
t.late = 8;
console.log(Object.keys(s).join(","), Object.keys(t).join(","), log.slice(-2).join(" "));
