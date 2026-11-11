// An Array's iteration reads every index below its length: a hole reads
// undefined. Each array here may hold one, so its loop element stays
// string | undefined; a dense one (literal, push) yields its strings only.
function show(name: string, xs: string[]): void {
  const out: string[] = [];
  for (const s of xs) out.push(typeof s === "string" ? s.trim() : "<" + typeof s + ">");
  console.log(name, out.join(","));
}
const lit = [" a", "b "];
show("lit", lit);
const pushed: string[] = [];
pushed.push(" p");
pushed.push("q ");
show("push", pushed);
const idx = [" x"];
idx[3] = "y ";
show("index", idx);
const len = ["l "];
len.length = 3;
show("length", len);
const del = [" d", "e"];
delete del[0];
show("delete", del);
const sized: string[] = new Array(2);
sized.push(" n");
show("new Array", sized);
const assigned = [" s"];
Object.assign(assigned, { 2: "t" });
show("assign", assigned);
const defined = [" u"];
Object.defineProperty(defined, "2", { value: "v", enumerable: true, writable: true, configurable: true });
show("define", defined);
const holey = [" h", , "i"];
show("elision", holey as string[]);
function grow(a: string[], i: number): void { a[i] = "g"; }
const viaFn = ["f "];
grow(viaFn, process.argv.length + 2);
show("callee", viaFn);
