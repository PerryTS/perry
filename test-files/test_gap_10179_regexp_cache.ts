// Node oracle: node --experimental-strip-types test-files/test_gap_10179_regexp_cache.ts
// Shared immutable programs must not share any observable RegExp state.
const source = "(?<word>a+)(?=b)";
const first = new RegExp(source, "gd");
const second = new RegExp(["(?<word>", "a+)(?=b)"].join(""), "dg");
console.log("display", first.source, first.flags, second.source, second.flags);
console.log("identity", first === second, RegExp(first) === first, new RegExp(first) === first);
console.log("test", first.test("xaab"), first.lastIndex, second.lastIndex);
console.log("exec", JSON.stringify(second.exec("xaab")), first.lastIndex, second.lastIndex);
console.log("reset", first.test("xaab"), first.lastIndex, second.lastIndex);
for (const flags of ["", "i", "g", "y", "gy", "u", "s", "m"]) {
    const re = new RegExp("a", flags);
    re.lastIndex = 1;
    console.log("flags", flags, re.test("bAab"), re.lastIndex, re.test("a"), re.lastIndex);
}
let hot = new RegExp("hot", "g");
for (let i = 0; i < 1100; i++) {
    const cold = new RegExp("cold" + i, "i");
    if (!cold.test("COLD" + i)) throw new Error("cold pattern changed");
    hot = new RegExp("hot", "g");
    if (hot.lastIndex !== 0 || !hot.test("hot")) throw new Error("shared lastIndex");
}
console.log("churn", hot.source, hot.flags, hot.lastIndex);

const compiled = new RegExp("before", "g");
const twin = new RegExp("before", "g");
compiled.lastIndex = 9;
console.log("compile", compiled.compile("after", "ig") === compiled, compiled.source, compiled.flags, compiled.lastIndex);
console.log("twin", twin.source, twin.flags, twin.lastIndex, twin.test("before"));
try { compiled.compile("(", ""); } catch (error) { console.log("invalid", error instanceof SyntaxError); }
console.log("unchanged", compiled.source, compiled.flags);
compiled.compile(twin);
console.log("copy", compiled.source, compiled.flags, compiled.lastIndex);
Object.defineProperty(compiled, "lastIndex", { writable: false });
try { compiled.compile("published", "y"); } catch (error) { console.log("readonly", error instanceof TypeError); }
console.log("published", compiled.source, compiled.flags, compiled.lastIndex);

const stateful = /a/g;
stateful.lastIndex = { valueOf() { console.log("coerce lastIndex"); return 1; } } as any;
console.log("coercion", stateful.test("ba"), stateful.lastIndex);
const overridden = /a/;
Object.defineProperty(overridden, "exec", { get() { console.log("get exec"); return () => null; } });
console.log("override", overridden.test("a"));
const originalExec = RegExp.prototype.exec;
RegExp.prototype.exec = function (value: string) {
    console.log("prototype exec", value);
    return originalExec.call(this, value);
};
console.log("patched", /a/.test("a"), "a".replace(/a/g, "x"));
RegExp.prototype.exec = originalExec;

const ansi = /\u001b\[[0-9;]*m/g;
console.log("replace", "plain".replace(ansi, ""), ansi.lastIndex);
console.log("replace-match", "\u001b[31mred\u001b[0m".replace(ansi, ""), ansi.lastIndex);
console.log("sticky", "baaa".replace(/a/gy, "x"), "aaab".replace(/a/gy, "x"));
console.log("empty", "🚀a".replace(/(?:)/gu, "-"));
console.log("captures", "ab ab".replace(/(a)(b)/g, "$2$1"));
console.log("remove-empty", "🚀a".replace(/(?:)/gu, ""));
const stickyRemove = /a/y;
stickyRemove.lastIndex = 1;
console.log("remove-sticky", "baa".replace(stickyRemove, ""), stickyRemove.lastIndex);
const originalFlags = Object.getOwnPropertyDescriptor(RegExp.prototype, "flags")!;
Object.defineProperty(RegExp.prototype, "flags", { configurable: true, get() { console.log("get flags"); return "g"; } });
console.log("flags-hook", "aa".replace(/a/g, ""));
Object.defineProperty(RegExp.prototype, "flags", originalFlags);
const originalReplace = RegExp.prototype[Symbol.replace];
Object.defineProperty(RegExp.prototype, Symbol.replace, { configurable: true, get() { console.log("get replace"); return originalReplace; } });
console.log("replace-hook", "aa".replace(/a/g, ""));
Object.defineProperty(RegExp.prototype, Symbol.replace, { configurable: true, writable: true, value: originalReplace });
const ownReplace = /a/g;
ownReplace[Symbol.replace] = function () { return "own replacement"; };
console.log("own-replace", "aa".replace(ownReplace, ""));
const changedProto = /a/g;
Object.setPrototypeOf(changedProto, { exec() { console.log("custom proto"); return null; } });
console.log("custom-proto", RegExp.prototype.test.call(changedProto, "a"));
for (const pattern of ["", "a/b", "\n", "\ud800", "🚀"]) {
    const a = new RegExp(pattern);
    const b = new RegExp(pattern);
    console.log("source", JSON.stringify(a.source), a.flags, a.test(pattern), b.test(pattern));
}
