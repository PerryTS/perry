// `a[0]` / `a[0] = v` / `a[i]` behind a `number[]` / `string[]` annotation given null or undefined: node throws a
// TypeError (RequireObjectCoercible / ToObject), in sloppy code too. Before, the annotated tier's cold read answered
// undefined and the store fallback returned silently. Forms: parameter, class field, local, element read in a loop.
// Keys: 0, an index, -1, 1.5, "0", "x". Negative controls: a real array, an empty array, a hole, an out-of-bounds
// index, a string behind the annotation.
function mk(k: string): any {
  if (k === "null") return JSON.parse("null");
  if (k === "undef") return JSON.parse("[]")[0];
  if (k === "arr") return [7, 8];
  if (k === "empty") return [];
  if (k === "holey") return [, 8];
  if (k === "str") return "ab";
  return 0;
}
function rd(a: number[]): any { return a[0]; }
function rdOob(a: number[]): any { return a[5]; }
function wr(a: number[]): any { a[0] = 9; return a[0]; }
function rdLoop(a: number[], n: number): any { let s = 0; for (let i = 0; i < n; i++) s += a[i]; return s; }
function rdStr(a: string[]): any { return a[1]; }
function rdKey(a: number[], k: any): any { return a[k]; }
class H {
  k: number[] = [1];
  rd(): any { return this.k[0]; }
  wr(): any { this.k[0] = 4; return this.k[0]; }
}
function local(v: any): any { const a: number[] = v; return a[0]; }
function show(f: () => any): string {
  try {
    return String(f());
  } catch (e: any) {
    return "THROW " + e.name;
  }
}
for (const k of ["null", "undef", "arr", "empty", "holey", "str"]) {
  const h = new H();
  h.k = mk(k);
  console.log(
    k,
    show(() => rd(mk(k))),
    show(() => rdOob(mk(k))),
    show(() => wr(mk(k))),
    show(() => rdLoop(mk(k), 1)),
    show(() => rdStr(mk(k))),
    show(() => h.rd()),
    show(() => h.wr()),
    show(() => local(mk(k))),
    show(() => rdKey(mk(k), -1)),
    show(() => rdKey(mk(k), 1.5)),
    show(() => rdKey(mk(k), "0")),
    show(() => rdKey(mk(k), "x")),
  );
}
