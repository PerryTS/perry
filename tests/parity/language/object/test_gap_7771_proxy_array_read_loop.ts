// #7771: a proxy-wrapped array behind a P[]-typed binding must take the
// generic path (the clone's brand guard declines) and match node.
// Kept alone in this file: #7775 tracks a module-wide miscompile triggered
// by the mere presence of `new Proxy(arr, {})` alongside other read loops.
class P { x: number; y: number; constructor(x: number, y: number) { this.x = x; this.y = y; } }
function build(n: number): P[] {
  const a: P[] = [];
  for (let i = 0; i < n; i++) a.push(new P(i, i + 1));
  return a;
}
function h5(): number {
  const raw = build(10);
  const a: P[] = new Proxy(raw, {}) as any;
  let s = 0;
  for (let i = 0; i < a.length; i++) { const r = a[i]; s += r.x + r.y; }
  return s;
}
console.log("h5", h5());
