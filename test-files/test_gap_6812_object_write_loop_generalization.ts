"use strict";

function fieldSum(objects: any[], fields: string[]): number {
  let sum = 0;
  for (let i = 0; i < objects.length; i++) {
    const object: any = objects[i];
    if (object === undefined) continue;
    for (let k = 0; k < fields.length; k++) {
      const value = object[fields[k]];
      if (typeof value === "number") sum += value;
    }
  }
  return sum;
}

// Eligible one-field clone.
const one: any[] = [
  { a: 0, b: 0, c: 0, d: 0 },
  { a: 0, b: 0, c: 0, d: 0 },
  { a: 0, b: 0, c: 0, d: 0 },
  { a: 0, b: 0, c: 0, d: 0 },
];
for (let r = 0; r < 7; r++) {
  for (let i = 0; i < 4; i++) {
    const object: any = one[i];
    object.c = r + i;
  }
}
console.log("one", fieldSum(one, ["c"]));

// Eligible four-field clone, including nested +/- expressions.
const four: any[] = [
  { a: 0, b: 0, c: 0, d: 0 },
  { a: 0, b: 0, c: 0, d: 0 },
  { a: 0, b: 0, c: 0, d: 0 },
  { a: 0, b: 0, c: 0, d: 0 },
];
for (let r = 0; r < 7; r++) {
  for (let i = 0; i < 4; i++) {
    const object: any = four[i];
    object.a = r + i;
    object.b = r - i;
    object.c = r + i + 1;
    object.d = r - i - 1;
  }
}
console.log("four", fieldSum(four, ["a", "b", "c", "d"]));

// Duplicate target slots are valid, but source order remains observable.
const duplicate: any[] = [{ x: 0, y: 0 }, { x: 0, y: 0 }];
for (let r = 0; r < 5; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = duplicate[i];
    object.x = r + i;
    object.x = r - i;
    object.y = r + i + 2;
  }
}
console.log("duplicate", fieldSum(duplicate, ["x", "y"]));

// Five fields are deliberately outside the bounded clone.
const five: any[] = [
  { a: 0, b: 0, c: 0, d: 0, e: 0 },
  { a: 0, b: 0, c: 0, d: 0, e: 0 },
];
for (let r = 0; r < 5; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = five[i];
    object.a = r + i;
    object.b = r - i;
    object.c = r + i + 1;
    object.d = r - i - 1;
    object.e = r + i + 2;
  }
}
console.log("five", fieldSum(five, ["a", "b", "c", "d", "e"]));

// Distinct shapes must fail the once-per-array proof before the first store.
const mixed: any[] = [{ x: 0, a: 1 }, { x: 0, b: 2 }];
for (let r = 0; r < 5; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = mixed[i];
    object.x = r + i;
  }
}
console.log("mixed", fieldSum(mixed, ["x"]));

let holeThrew = false;
const hole: any[] = [{ x: 0 }, , { x: 0 }];
try {
  for (let r = 0; r < 1; r++) {
    for (let i = 0; i < 3; i++) {
      const object: any = hole[i];
      object.x = r + i;
    }
  }
} catch (error) {
  holeThrew = error instanceof TypeError;
}
console.log("hole", holeThrew, fieldSum(hole, ["x"]));

let shortThrew = false;
const short: any[] = [{ x: 0 }, { x: 0 }];
try {
  for (let r = 0; r < 1; r++) {
    for (let i = 0; i < 3; i++) {
      const object: any = short[i];
      object.x = r + i;
    }
  }
} catch (error) {
  shortThrew = error instanceof TypeError;
}
console.log("short", shortThrew, fieldSum(short, ["x"]));

// Dynamic property growth beyond INLINE_SLOT_FLOOR remains overflow storage.
const overflow: any[] = [];
for (let i = 0; i < 2; i++) {
  const object: any = { a: 0, b: 0, c: 0, d: 0 };
  object.x = i;
  overflow.push(object);
}
for (let r = 0; r < 5; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = overflow[i];
    object.x = r + i;
  }
}
console.log("overflow", fieldSum(overflow, ["x"]));

// JSON.parse creates class-id-zero regular objects, which the guard rejects.
const parsed: any[] = [JSON.parse('{"x":0}'), JSON.parse('{"x":0}')];
for (let r = 0; r < 5; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = parsed[i];
    object.x = r + i;
  }
}
console.log("parsed", fieldSum(parsed, ["x"]));

let readonlyThrew = false;
const readonly: any[] = [{ x: 1 }, { x: 2 }];
Object.defineProperty(readonly[0], "x", { value: 1, writable: false });
Object.defineProperty(readonly[1], "x", { value: 2, writable: false });
try {
  for (let r = 0; r < 1; r++) {
    for (let i = 0; i < 2; i++) {
      const object: any = readonly[i];
      object.x = r + i;
    }
  }
} catch (error) {
  readonlyThrew = error instanceof TypeError;
}
console.log("readonly", readonlyThrew, fieldSum(readonly, ["x"]));

let frozenThrew = false;
const frozen: any[] = [Object.freeze({ x: 1 }), Object.freeze({ x: 2 })];
try {
  for (let r = 0; r < 1; r++) {
    for (let i = 0; i < 2; i++) {
      const object: any = frozen[i];
      object.x = r + i;
    }
  }
} catch (error) {
  frozenThrew = error instanceof TypeError;
}
console.log("frozen", frozenThrew, fieldSum(frozen, ["x"]));

// Sealed/non-extensible existing writable fields still update semantically,
// but both mutable-state flags intentionally reject the raw clone.
const sealed: any[] = [Object.seal({ x: 0 }), Object.seal({ x: 0 })];
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = sealed[i];
    object.x = r + i;
  }
}
console.log("sealed", fieldSum(sealed, ["x"]));

const noExtend: any[] = [
  Object.preventExtensions({ x: 0 }),
  Object.preventExtensions({ x: 0 }),
];
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = noExtend[i];
    object.x = r + i;
  }
}
console.log("noextend", fieldSum(noExtend, ["x"]));

let setterSum = 0;
const accessors: any[] = [{}, {}];
for (let i = 0; i < accessors.length; i++) {
  Object.defineProperty(accessors[i], "x", {
    set(value: number) {
      setterSum += value;
    },
  });
}
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = accessors[i];
    object.x = r + i;
  }
}
console.log("accessor", setterSum);

let inheritedSetterSum = 0;
const inheritedPrototype: any = {};
Object.defineProperty(inheritedPrototype, "x", {
  set(value: number) {
    inheritedSetterSum += value;
  },
});
const inherited: any[] = [
  Object.create(inheritedPrototype),
  Object.create(inheritedPrototype),
];
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = inherited[i];
    object.x = r + i;
  }
}
console.log("inherited", inheritedSetterSum);

let proxySetterSum = 0;
const proxyTargets: any[] = [{ x: 0 }, { x: 0 }];
const proxies: any[] = [
  new Proxy(proxyTargets[0], {
    set(target: any, property: string, value: any, receiver: any) {
      proxySetterSum += value;
      return Reflect.set(target, property, value, receiver);
    },
  }),
  new Proxy(proxyTargets[1], {
    set(target: any, property: string, value: any, receiver: any) {
      proxySetterSum += value;
      return Reflect.set(target, property, value, receiver);
    },
  }),
];
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = proxies[i];
    object.x = r + i;
  }
}
console.log("proxy", proxySetterSum, fieldSum(proxyTargets, ["x"]));

// Pointer-capable, allocating, calling, and dynamic-key RHS/key forms retain
// their original rooted/barrier/coercion semantics.
const pointer: any = { value: 9 };
const pointerObjects: any[] = [{ x: null }, { x: null }];
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = pointerObjects[i];
    object.x = pointer;
  }
}
console.log(
  "pointer",
  pointerObjects[0].x.value + pointerObjects[1].x.value,
);

const allocated: any[] = [{ x: null }, { x: null }];
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = allocated[i];
    object.x = { value: r + i };
  }
}
console.log(
  "allocated",
  allocated[0].x.value + allocated[1].x.value,
);

const producers: any[] = [(left: number, right: number) => left + right];
const produce: any = producers[0];
const called: any[] = [{ x: 0 }, { x: 0 }];
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = called[i];
    object.x = produce(r, i);
  }
}
console.log("called", fieldSum(called, ["x"]));

const dynamic: any[] = [{ x: 0 }, { x: 0 }];
const key = "x";
for (let r = 0; r < 3; r++) {
  for (let i = 0; i < 2; i++) {
    const object: any = dynamic[i];
    object[key] = r + i;
  }
}
console.log("dynamic", fieldSum(dynamic, ["x"]));
