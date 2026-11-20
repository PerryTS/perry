// #10637: reflective collection methods accept the backing of a class instance.
function exercise() {
  class M extends Map {}
  class S extends Set {}
  const m = new M([['a', 1]]);
  const s = new S(['a']);
  console.log('initial', m.get('a'), m.size, s.has('a'), s.size);
  console.log('map set', Map.prototype.set.call(m, 'b', 2) === m, m.get('b'));
  console.log('set add', Set.prototype.add.call(s, 'b') === s, s.has('b'));
  console.log('apply', Reflect.apply(Map.prototype.get, m, ['a']), Reflect.apply(Set.prototype.has, s, ['a']));
  const get = m.get;
  const has = s.has;
  console.log('extracted', get.call(m, 'a'), has.call(s, 'b'));
  const mapSize = Object.getOwnPropertyDescriptor(Map.prototype, 'size')!.get!;
  const setSize = Object.getOwnPropertyDescriptor(Set.prototype, 'size')!.get!;
  console.log('size getters', mapSize.call(m), setSize.call(s));
  console.log('map keys', Array.from(Map.prototype.keys.call(m)).join(','));
  console.log('map values', Array.from(Map.prototype.values.call(m)).join(','));
  console.log('map entries', Array.from(Map.prototype.entries.call(m)).map((p: any) => p.join(':')).join(','));
  console.log('set keys', Array.from(Set.prototype.keys.call(s)).join(','));
  console.log('set values', Array.from(Set.prototype.values.call(s)).join(','));
  console.log('set entries', Array.from(Set.prototype.entries.call(s)).map((p: any) => p.join(':')).join(','));
  const context = { tag: 'callback' };
  Map.prototype.forEach.call(m, function (this: any, value: any, key: any, receiver: any) {
    console.log('map callback', key, value, receiver === m, this === context);
  }, context);
  Set.prototype.forEach.call(s, function (this: any, value: any, key: any, receiver: any) {
    console.log('set callback', key, value, receiver === s, this === context);
  }, context);
  console.log('delete', Map.prototype.delete.call(m, 'a'), Set.prototype.delete.call(s, 'a'));
  console.log('missing', Map.prototype.has.call(m, 'a'), Set.prototype.has.call(s, 'a'));
  console.log('clear', Map.prototype.clear.call(m), Set.prototype.clear.call(s), m.size, s.size);
  class CustomMap extends M { get(key: any) { return 'own:' + key; } }
  const custom = new CustomMap([['x', 3]]);
  console.log('override', custom.get('x'), Map.prototype.get.call(custom, 'x'));
  const invalid = [s, {}, Object.create(m), new Proxy(m, {})];
  for (const receiver of invalid) {
    try { Map.prototype.get.call(receiver, 'x'); console.log('unexpected map success'); }
    catch (e: any) { console.log('map brand', e.name); }
  }
  const invalidSets = [m, {}, Object.create(s), new Proxy(s, {})];
  for (const receiver of invalidSets) {
    try { Set.prototype.has.call(receiver, 'x'); console.log('unexpected set success'); }
    catch (e: any) { console.log('set brand', e.name); }
  }
}
exercise();
// Native collections continue to use their registered backing directly.
const plainMap = new Map();
const plainSet = new Set();
console.log('plain identity', Map.prototype.set.call(plainMap, 'a', 5) === plainMap, Set.prototype.add.call(plainSet, 'a') === plainSet);
console.log('plain reads', Map.prototype.get.call(plainMap, 'a'), Set.prototype.has.call(plainSet, 'a'));
