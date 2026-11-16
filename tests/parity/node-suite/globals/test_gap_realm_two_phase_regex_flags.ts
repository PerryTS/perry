// The realm entries made hot: regex flag reads and searches (the runtime
// asks for %RegExp.prototype% through the realm's core entry on each), and
// reads of the global object as a value (the full entry). A large program
// makes 10^4 of these while it starts; with or without the two-phase realm
// one such read must not cost more than it did (PR-realm2-2.md, "the
// Supabase gate").
// A member added to %RegExp.prototype% (a polyfill does this): from here on
// each match and search asks the realm for the prototype to check that the
// flag accessors are still the builtin ones.
(RegExp.prototype as any).describe = function (this: RegExp) { return this.source; };
const patterns = [/a/g, /b/i, /c+/m, /d/y, /e/s, /f/u, /g/gi, /h/];
let flags = 0, hits = 0, globals = 0;
for (let i = 0; i < 20000; i++) {
  const re = patterns[i % patterns.length];
  if (re.global) flags++;
  if (re.ignoreCase) flags += 2;
  if (re.multiline) flags += 3;
  if (re.sticky) flags += 4;
  flags += re.flags.length;
  if ("xaybzcqdwevfugth".search(re) >= 0) hits++;
  if ("abcdefgh".match(re) !== null) hits++;
  const g: any = globalThis;
  if (g.Math === Math) globals++;
  if (typeof g.undeclaredRealmName === "undefined") globals++;
}
console.log("regex flags", flags, hits, globals, (patterns[2] as any).describe());
