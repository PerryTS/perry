// Object.assign copies a computed source key with [[Set]], so an inherited
// setter whose name the key spells runs with the target as `this`.
function source(n: number): any {
  const s: any = {};
  for (let i = 0; i < n; i++) s["s" + i] = i * 10;
  return s;
}

const seen: string[] = [];
const proto = {
  set s20(v: number) {
    seen.push("proto s20=" + v);
  },
};
const fromLiteral: any = Object.create(proto);
Object.assign(fromLiteral, source(40));
console.log(seen.join(","), Object.hasOwn(fromLiteral, "s20"), fromLiteral.s39);

class WithSetter {
  set s7(v: number) {
    seen.push("class s7=" + v + " " + (this instanceof WithSetter));
  }
}
seen.length = 0;
const instance: any = new WithSetter();
Object.assign(instance, { ...source(10) });
console.log(seen.join(","), Object.hasOwn(instance, "s7"), Object.keys(instance).length);

seen.length = 0;
const late: any = new WithSetter();
const filled: any = {};
const copy = () => Object.assign(late, filled);
for (let i = 0; i < 12; i++) filled["s" + i] = -i;
copy();
console.log(seen.join(","), late.s11, Object.keys(late).join(" "));
