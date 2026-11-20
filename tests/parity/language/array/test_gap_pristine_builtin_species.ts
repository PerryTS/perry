// #11915: Symbol.species stays observable, before and after a prototype write.
class Plain extends Array<number> {
  static get [Symbol.species]() { return Array; }
}
function run() {
  const p = Plain.from([1, 2, 3]) as Plain;
  const mapped = p.map((x) => x * 2);
  console.log(mapped instanceof Plain, Array.isArray(mapped), mapped.join(','));
  const a = [1, 2, 3];
  console.log(a.map((x) => x + 1).join(','), a.filter((x) => x > 1).join(','), a.slice(1).join(','));
}
run();
// An unrelated write withdraws the fact; species and results must not change.
(Array.prototype as any).speciesProbe = 1;
run();
console.log(Object.keys([5]).join(','), 'speciesProbe' in []);
