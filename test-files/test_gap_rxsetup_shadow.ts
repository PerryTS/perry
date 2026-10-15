// exec and Symbol.replace defined on Object.prototype must change nothing
// (RegExp.prototype owns both keys); the same keys redefined on
// RegExp.prototype must be seen by no-match and match replaces, g and non-g.
const show = (label: string, value: unknown) => console.log(label, JSON.stringify(value));
const log: string[] = [];
const subjects = ["abc", "a+b+c", ""];
const run = (label: string) => {
  for (const s of subjects) {
    const g = /\+/g;
    const n = /\+/;
    g.lastIndex = 2;
    show(label + " [" + s + "]", [s.replace(g, "_"), g.lastIndex, s.replace(n, "_"), n.lastIndex, log.splice(0)]);
  }
};
run("pristine");

// Object.prototype definitions: no effect.
Object.defineProperty(Object.prototype, "exec", {
  configurable: true, writable: true, value: function () { log.push("object exec"); return null; },
});
Object.defineProperty(Object.prototype, Symbol.replace, {
  configurable: true, writable: true, value: function () { log.push("object replace"); return "object"; },
});
run("object proto");
delete (Object.prototype as any).exec;
delete (Object.prototype as any)[Symbol.replace];
run("object proto removed");

// RegExp.prototype exec override that delegates.
const execDesc = Object.getOwnPropertyDescriptor(RegExp.prototype, "exec")!;
const exec = RegExp.prototype.exec;
RegExp.prototype.exec = function (this: RegExp, s: string) { log.push("proto exec " + s); return exec.call(this, s); };
run("proto exec");
Object.defineProperty(RegExp.prototype, "exec", execDesc);
run("proto exec restored");

// RegExp.prototype exec that never matches.
RegExp.prototype.exec = function (this: RegExp, s: string) { log.push("null exec " + s); return null; };
run("proto exec null");
Object.defineProperty(RegExp.prototype, "exec", execDesc);

// RegExp.prototype Symbol.replace override.
const replDesc = Object.getOwnPropertyDescriptor(RegExp.prototype, Symbol.replace)!;
Object.defineProperty(RegExp.prototype, Symbol.replace, {
  configurable: true, writable: true,
  value: function (s: string, r: string) { log.push("proto replace " + s); return "patched:" + s; },
});
run("proto symbol replace");
Object.defineProperty(RegExp.prototype, Symbol.replace, replDesc);
run("restored");
