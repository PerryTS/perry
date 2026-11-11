// VR-3a (DESIGN-p4l4 §5.1): a record binding that may be `undefined` has a
// presence lane. Presence tests read it; an absent read throws as on
// `undefined`; a boundary takes a copy or `undefined`.
// vr-expect: ox,oy ADMITTED
function run(c: boolean) {
  const o = c ? undefined : { ox: 3, oy: 4 };
  console.log(o === undefined, o !== undefined, o == null, o != null, typeof o);
  console.log(o ? o.ox : -1, o?.oy, o && o.ox, !o);
  console.log(o, o || "none", o ?? "nil");
  try {
    console.log((o as { ox: number }).ox);
  } catch (e) {
    console.log("threw", e instanceof TypeError);
  }
}
run(false);
run(true);
