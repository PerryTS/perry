// #10446: a `Set`/`Map`-typed value holding `undefined` (or null, or a
// primitive) reached the direct js_set_*/js_map_* fast path with no receiver
// check, so the process died with SIGSEGV instead of throwing a catchable
// TypeError. Node's message text for a non-nullish receiver names the source
// expression (`h.set.add is not a function`), which perry does not reproduce
// on any path, so those rows print the error CLASS only.

function nullish(label: string, fn: () => unknown): void {
  try {
    fn();
    console.log(label, 'no throw');
  } catch (e: unknown) {
    const err = e as Error;
    console.log(label, err.constructor.name, err.message);
  }
}

function threw(label: string, fn: () => unknown): void {
  try {
    fn();
    console.log(label, 'no throw');
  } catch (e: unknown) {
    const err = e as Error;
    console.log(label, 'threw', err.constructor.name);
  }
}

class Holder {
  set: Set<string> = new Set();
  map: Map<string, number> = new Map();
  add(x: string): void {
    this.set.add(x);
  }
  has(x: string): boolean {
    return this.set.has(x);
  }
  del(x: string): boolean {
    return this.set.delete(x);
  }
  clearSet(): void {
    this.set.clear();
  }
  setSize(): number {
    return this.set.size;
  }
  put(k: string, v: number): void {
    this.map.set(k, v);
  }
  get(k: string): number | undefined {
    return this.map.get(k);
  }
  hasKey(k: string): boolean {
    return this.map.has(k);
  }
  delKey(k: string): boolean {
    return this.map.delete(k);
  }
  clearMap(): void {
    this.map.clear();
  }
  mapSize(): number {
    return this.map.size;
  }
  eachSet(): void {
    this.set.forEach(() => {});
  }
  eachMap(): void {
    this.map.forEach(() => {});
  }
}

// The working shape first: a real Set/Map must behave exactly as before.
const ok = new Holder();
ok.add('a');
ok.put('k', 1);
console.log('works:', ok.has('a'), ok.get('k'), ok.setSize(), ok.mapSize(), ok.del('a'), ok.delKey('k'));

// Field receivers holding undefined / null.
for (const bad of [undefined, null]) {
  const h = new Holder();
  (h as any).set = bad;
  (h as any).map = bad;
  const tag = bad === null ? 'null' : 'undefined';
  nullish(`field set.add ${tag}:`, () => h.add('x'));
  nullish(`field set.has ${tag}:`, () => h.has('x'));
  nullish(`field set.delete ${tag}:`, () => h.del('x'));
  nullish(`field set.clear ${tag}:`, () => h.clearSet());
  nullish(`field set.size ${tag}:`, () => h.setSize());
  nullish(`field set.forEach ${tag}:`, () => h.eachSet());
  nullish(`field map.set ${tag}:`, () => h.put('x', 1));
  nullish(`field map.get ${tag}:`, () => h.get('x'));
  nullish(`field map.has ${tag}:`, () => h.hasKey('x'));
  nullish(`field map.delete ${tag}:`, () => h.delKey('x'));
  nullish(`field map.clear ${tag}:`, () => h.clearMap());
  nullish(`field map.size ${tag}:`, () => h.mapSize());
  nullish(`field map.forEach ${tag}:`, () => h.eachMap());
}

// Local bindings whose declared type is Set/Map.
function localSet(value: unknown, tag: string): void {
  const s: Set<string> = value as Set<string>;
  nullish(`local set.add ${tag}:`, () => s.add('x'));
  nullish(`local set.has ${tag}:`, () => s.has('x'));
  nullish(`local set.delete ${tag}:`, () => s.delete('x'));
  nullish(`local set.clear ${tag}:`, () => s.clear());
}

function localMap(value: unknown, tag: string): void {
  const m: Map<string, number> = value as Map<string, number>;
  nullish(`local map.set ${tag}:`, () => m.set('x', 1));
  nullish(`local map.get ${tag}:`, () => m.get('x'));
  nullish(`local map.has ${tag}:`, () => m.has('x'));
  nullish(`local map.delete ${tag}:`, () => m.delete('x'));
  nullish(`local map.clear ${tag}:`, () => m.clear());
}

localSet(undefined, 'undefined');
localSet(null, 'null');
localMap(undefined, 'undefined');
localMap(null, 'null');

// Wrong-type receivers: a number, a string and a boolean can carry no
// collection method, so every engine throws a TypeError. Only the class is
// compared (see the header note about Node's expression-naming message).
function wrongType(value: unknown, tag: string): void {
  const s: Set<string> = value as Set<string>;
  const m: Map<string, number> = value as Map<string, number>;
  threw(`local set.add ${tag}:`, () => s.add('x'));
  threw(`local set.has ${tag}:`, () => s.has('x'));
  threw(`local map.get ${tag}:`, () => m.get('x'));
  threw(`local map.set ${tag}:`, () => m.set('x', 1));
  const h = new Holder();
  (h as any).set = value;
  (h as any).map = value;
  threw(`field set.add ${tag}:`, () => h.add('x'));
  threw(`field map.get ${tag}:`, () => h.get('x'));
}

wrongType(42, 'number');
wrongType('str', 'string');
wrongType(true, 'boolean');

// A method call on a genuine Set/Map reached through the same typed paths
// after all the throwing above still works.
const after = new Holder();
after.add('z');
after.put('z', 26);
console.log('still works:', after.has('z'), after.get('z'), after.setSize(), after.mapSize());

// Number- and string-keyed fast paths (the typed helper variants) on a
// nullish receiver take the same guard.
function typedKeys(): void {
  const numKeys: Map<number, number> = undefined as unknown as Map<number, number>;
  const strSet: Set<string> = undefined as unknown as Set<string>;
  const numSet: Set<number> = undefined as unknown as Set<number>;
  nullish('number-key map.set:', () => numKeys.set(1, 2));
  nullish('number-key map.get:', () => numKeys.get(1));
  nullish('string set.add:', () => strSet.add('s'));
  nullish('number set.add:', () => numSet.add(1));
  nullish('number set.has:', () => numSet.has(1));
}
typedKeys();
