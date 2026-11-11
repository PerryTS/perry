// DESIGN-p4l4 §2.2: presence tests observe no identity.
// vr-expect: px,py ADMITTED
function mk(x: number, y: number) {
  return { px: x, py: y };
}
function find(n: number): { px: number; py: number } | undefined {
  return n > 0 ? mk(n, n * 2) : undefined;
}
for (const n of [0, 1, 2]) {
  const r = find(n);
  console.log(r === undefined, r !== undefined, r?.px, r ? r.py : -1, typeof r, r == null);
}
