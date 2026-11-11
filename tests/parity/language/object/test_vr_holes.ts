// DESIGN-p4l4 R6(c): record arrays have no holes; until the VR-4b
// intrinsics every record held by an array rejects.
// vr-expect: ha,hb REJECTED R6
function mk(a: number, b: number) {
  return { ha: a, hb: b };
}
const a: { ha: number; hb: number }[] = [];
a[3] = mk(1, 2);
a.length = 5;
console.log(a.length, a[3], a[0], a);
