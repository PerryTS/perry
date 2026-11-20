// Keep this body above the cross-module inline budget: the linked export ABI
// must be exercised, not replaced by an inlined copy of the correct body.
export function $n() {
  const values = [];
  values.push(1);
  values.push(2);
  values.push(3);
  values.push(4);
  values.push(5);
  values.push(6);
  values.push(7);
  values.push(8);
  values.push(9);
  values.push(10);
  values.push(11);
  return values.length;
}

export function _n(map) {
  let total = 0;
  map.forEach(value => { total += value; });
  return total;
}

export function $wide(a, b, c, d, e, f) {
  return $n() + a + b + c + d + e + f;
}

export function _wide() { return -1; }
export { $n as $renamed, $wide as $renamedWide };
