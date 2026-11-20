// DESIGN-p4l4 §4.1 (finding 14): JSON.stringify is a sanctioned boundary
// without a replacer, or with a literal null/undefined/array one.
// vr-expect: ja,jb ADMITTED
// vr-expect: ka,kb REJECTED R5Protocol(JsonReplacer)
function mkj(a: number, b: boolean) {
  return { ja: a, jb: b };
}
function mkk(a: number, b: boolean) {
  return { ka: a, kb: b };
}
const j = mkj(1, true);
console.log(JSON.stringify(j), JSON.stringify(j, null, 2), JSON.stringify(j, ["ja"]));
const k = mkk(2, false);
console.log(JSON.stringify(k, (key, v) => (typeof v === "number" ? v * 10 : v)));
