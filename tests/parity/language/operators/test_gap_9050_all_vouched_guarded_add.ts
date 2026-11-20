// The guarded `+` lowering's all-vouched corner (found compiling pi + the cc
// bundle on 2026-08-29): `width += eaw(c)` where the CALL leaf is flagged
// declared-only-numeric (routing the tree into the guarded lowering) while
// integer-literal returns make every leaf provenance-vouched (nothing left
// to test). The lowering used to bail! on that corner — an ICE that killed
// both application builds — and must instead emit the unguarded fast tree.
// The for-of loop and codePointAt shapes are load-bearing: without the loop
// the tree never routes into the guarded lowering at all.
function eaw(cp: number): number {
  if (cp > 100) { return 2; }
  return 1;
}
function gw(s: string): number {
  let width = eaw(s.codePointAt(0)!);
  for (const ch of s) {
    const c = ch.codePointAt(0)!;
    if (c >= 65280) { width += eaw(c); }
    else if (c === 3635) { width += 1; }
  }
  return width;
}
console.log(gw("ａb"));
console.log(gw("abc"));
console.log(gw("aี"));
