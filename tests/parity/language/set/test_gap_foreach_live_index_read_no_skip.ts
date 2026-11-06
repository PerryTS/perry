// A forEach callback that deletes already-visited entries and then performs an
// array-like indexed read on the collection (`set[0]` / `map[0]` — Perry's
// live-index accessor, which squeezes tombstones so raw index == live index)
// must not shift the raw layout under the walk's own counter: every entry not
// deleted before its visit is visited exactly once. Node has no array-like
// read on collections (it yields undefined), so only visit ORDER is printed.
function walkSet(): string {
  const s = new Set<number>();
  for (let i = 0; i < 20; i++) s.add(i);
  const seen: number[] = [];
  s.forEach((v) => {
    seen.push(v);
    if (v === 5) { s.delete(1); s.delete(2); void (s as any)[0]; void (s as any)[1]; }
  });
  return `set ${seen.length} ${seen.join(",")} size=${s.size}`;
}
function walkMap(): string {
  const m = new Map<number, number>();
  for (let i = 0; i < 20; i++) m.set(i, i * 10);
  const seen: number[] = [];
  m.forEach((_v, k) => {
    seen.push(k);
    if (k === 5) { m.delete(1); m.delete(2); void (m as any)[0]; void (m as any)[1]; }
  });
  return `map ${seen.length} ${seen.join(",")} size=${m.size}`;
}
// Nested walks: the inner walk's read must not squeeze under the outer walk.
function nested(): string {
  const m = new Map<number, number>();
  for (let i = 0; i < 12; i++) m.set(i, i);
  const outer: number[] = [];
  m.forEach((_v, k) => {
    outer.push(k);
    if (k === 4) {
      m.delete(0);
      m.forEach((_iv, ik) => { if (ik === 6) void (m as any)[0]; });
    }
  });
  return `nested ${outer.length} ${outer.join(",")}`;
}
console.log(walkSet());
console.log(walkMap());
console.log(nested());
