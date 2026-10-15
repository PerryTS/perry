// Writes to RegExp.prototype seen by later no-match replaces, tests and
// splits on fresh literals. Each row restores the original afterwards.
const show = (label: string, value: unknown) => console.log(label, JSON.stringify(value));
const log: string[] = [];
const run = (label: string) => {
  const r = /\+/g;
  const plain = /\+/;
  show(label, ["abc".replace(r, "_"), r.lastIndex, plain.test("abc"), "abc".split(/\+/), log.splice(0)]);
};
run("pristine");

const exec = RegExp.prototype.exec;
RegExp.prototype.exec = function (this: RegExp, s: string) { log.push("proto exec " + s); return exec.call(this, s); };
run("proto exec");
RegExp.prototype.exec = exec;
run("exec restored");

const replace = RegExp.prototype[Symbol.replace];
(RegExp.prototype as any)[Symbol.replace] = function (s: string) { log.push("proto replace " + s); return "patched"; };
run("proto symbol replace");
(RegExp.prototype as any)[Symbol.replace] = replace;

for (const name of ["flags", "global", "sticky", "unicode", "hasIndices"]) {
  const desc = Object.getOwnPropertyDescriptor(RegExp.prototype, name)!;
  Object.defineProperty(RegExp.prototype, name, {
    configurable: true,
    get() { log.push("proto get " + name); return (desc.get as any).call(this); },
  });
  run("proto getter " + name);
  Object.defineProperty(RegExp.prototype, name, desc);
}

const flags = Object.getOwnPropertyDescriptor(RegExp.prototype, "flags")!;
delete (RegExp.prototype as any).flags;
{
  const r = /\+/g;
  r.lastIndex = 5;
  show("flags deleted", ["abc".replace(r, "_"), r.lastIndex, "a+b+c".replace(/\+/g, "_")]);
}
Object.defineProperty(RegExp.prototype, "flags", flags);
run("restored");
