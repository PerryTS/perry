// #13485: own properties on built-in iterator objects must not clobber their
// internal state (backing collection, cursor, kind).

function tag(o: any, i: number): any {
  o.a = i;
  o.b = i + 1;
  o["c" + i] = 9;
  return o;
}

function makeIterators(): any[] {
  return [
    [1, 2].values(),
    [1, 2].entries(),
    [1, 2].keys(),
    new Map([[1, 2]]).entries(),
    new Map([[1, 2]]).keys(),
    new Map([[1, 2]]).values(),
    new Set([5]).values(),
    new Set([5]).entries(),
    "xy"[Symbol.iterator](),
    new Uint8Array([4, 5]).values(),
    "a1a2".matchAll(/a(\d)/g),
    [1, 2, 3].values().map((x: number) => x * 2),
    [1, 2, 3].values().filter((x: number) => x > 1),
    [1, 2, 3].values().take(2),
    [1, 2, 3].values().drop(1),
    [1, 2, 3].values().flatMap((x: number) => [x, x]),
  ];
}

// Repeat so property-write inline caches are warm on later rounds.
for (let i = 0; i < 3; i++) {
  for (const it of makeIterators()) {
    tag(it, i);
    const r = it.next();
    console.log(JSON.stringify(r), it.a, it.b, it["c" + i], Object.keys(it).join(","));
    console.log(JSON.stringify([...it]));
  }
}

const deleted: any = [1, 2].values();
deleted.x = 1;
delete deleted.x;
deleted.y = 2;
console.log([...deleted].join(","), deleted.y, Object.keys(deleted).join(","));

const many: any = new Set([1, 2]).values();
for (let j = 0; j < 20; j++) many["p" + j] = j;
console.log([...many].join(","), many.p0, many.p19);
