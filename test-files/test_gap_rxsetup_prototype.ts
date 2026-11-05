// Writes to RegExp.prototype seen by later no-match replaces, tests and
// splits on fresh literals. Each row restores the original afterwards.
// DEC 250 locks RegExp.prototype: the regexps adopt a user prototype URP
// below it, and the writes go there.
const URP: any = Object.create(RegExp.prototype);
const rx = (r: RegExp): RegExp => Object.setPrototypeOf(r, URP);
const show = (label: string, value: unknown) => console.log(label, JSON.stringify(value));
const log: string[] = [];
const run = (label: string) => {
  const r = rx(/\+/g);
  const plain = rx(/\+/);
  show(label, ["abc".replace(r, "_"), r.lastIndex, plain.test("abc"), "abc".split(rx(/\+/)), log.splice(0)]);
};
run("pristine");

const exec = RegExp.prototype.exec;
URP.exec = function (this: RegExp, s: string) { log.push("proto exec " + s); return exec.call(this, s); };
run("proto exec");
URP.exec = exec;
run("exec restored");

const replace = RegExp.prototype[Symbol.replace];
URP[Symbol.replace] = function (s: string) { log.push("proto replace " + s); return "patched"; };
run("proto symbol replace");
URP[Symbol.replace] = replace;

for (const name of ["flags", "global", "sticky", "unicode", "hasIndices"]) {
  const desc = Object.getOwnPropertyDescriptor(RegExp.prototype, name)!;
  Object.defineProperty(URP, name, {
    configurable: true,
    get() { log.push("proto get " + name); return (desc.get as any).call(this); },
  });
  run("proto getter " + name);
  Object.defineProperty(URP, name, desc);
}

const flags = Object.getOwnPropertyDescriptor(RegExp.prototype, "flags")!;
// URP shadows the inherited flags getter with an undefined value (the read a
// deleted RegExp.prototype.flags gives).
Object.defineProperty(URP, "flags", { configurable: true, value: undefined });
{
  const r = rx(/\+/g);
  r.lastIndex = 5;
  show("flags deleted", ["abc".replace(r, "_"), r.lastIndex, "a+b+c".replace(rx(/\+/g), "_")]);
}
delete URP.flags;
run("restored");
