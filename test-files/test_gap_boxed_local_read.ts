// Reads of a boxed local (captured by a closure) in the function that declares it:
// reassigned parameters, lets written by closures, TDZ reads, reads before a hoisted box exists.
function walk(node: any): number[] {
  const seen: number[] = [];
  const peek = () => (node ? node.v : -1);
  while (node) { seen.push(node.v, peek()); node = node.next; }
  seen.push(peek());
  return seen;
}
console.log(walk({ v: 1, next: { v: 2, next: null } }).join(","));

function counter() {
  let n = 0;
  const inc = () => { n += 2; };
  const out: number[] = [];
  for (let i = 0; i < 4; i++) { inc(); out.push(n); }
  n = -1; out.push(n); inc(); out.push(n);
  return out;
}
console.log(counter().join(","));

function tdz(): string {
  const read = () => value;
  let r: string;
  try { r = String(read()); } catch (e: any) { r = e instanceof ReferenceError ? "ReferenceError" : "other"; }
  let value = "set";
  return r + "/" + read() + "/" + value;
}
console.log(tdz());

function tdzDirect(): string {
  try {
    // @ts-ignore
    const v = later;
    return "no throw " + v;
  } catch (e: any) {
    return e instanceof ReferenceError ? "ReferenceError" : "other";
  } finally {
    // keep `later` boxed
  }
  let later = 1;
  const f = () => later;
  f();
}
console.log(tdzDirect());

function varHoist(flag: boolean): any {
  if (flag) { var h: any = "assigned"; }
  const g = () => h;
  return [h, g()];
}
console.log(JSON.stringify(varHoist(false)), JSON.stringify(varHoist(true)));

function types(): string {
  let x: any = 1;
  const set = (v: any) => { x = v; };
  const out: string[] = [];
  for (const v of [2.5, "str", null, undefined, { a: 1 }, [1, 2], true, 10n]) {
    set(v);
    out.push(typeof x + ":" + String(x));
  }
  return out.join(" ");
}
console.log(types());

function loopClosures(): number[] {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i++) { let j = i * 10; fs.push(() => j); j += 1; }
  return fs.map((f) => f());
}
console.log(loopClosures().join(","));
