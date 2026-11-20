"use strict";

interface Point {
  x: number;
  y: number;
  negZero: number;
  nan: number;
  wide: number;
}

interface AnyBox {
  value: any;
}

function overwriteDynamic(p: any): void {
  p["y"] = 6.25;
}

function mutateCallback(p: any): void {
  p.x = { label: "callback" };
}

const p: Point = {
  x: 3.5,
  y: 4.25,
  negZero: -0,
  nan: NaN,
  wide: 2147483648.5,
};

const before = JSON.stringify(p);
const { x, y } = p;
const spread = { ...p };

console.log(before);
console.log(Number.isNaN(p.nan));
console.log(Object.is(p.negZero, -0));
console.log(p.wide);
console.log(x + y);
console.log(JSON.stringify(spread));

overwriteDynamic(p);
console.log(p.y);

mutateCallback(p);
console.log(JSON.stringify(p));
console.log((p as any).x.label);

const anyBox: AnyBox = { value: 7.75 };
console.log(JSON.stringify(anyBox));
(anyBox as any).value = { label: "boxed" };
console.log(JSON.stringify(anyBox));

class Counter {
  value: number = 1.5;
  other: any = "boxed";

  bump(delta: number): number {
    this.value = this.value + delta;
    return this.value;
  }
}

const c = new Counter();
console.log(c.bump(2.25));
(c as any).value = { label: "class-transition" };
console.log(JSON.stringify(c));
console.log((c as any).value.label);

const shortStringCounter = new Counter();
(shortStringCounter as any).value = "abc";
console.log(JSON.stringify(shortStringCounter));
console.log((shortStringCounter as any).value);

class GuardedCounter {
  value: number = 9.5;
}

const frozen = new GuardedCounter();
Object.freeze(frozen);
try {
  frozen.value = 14.5;
  console.log("frozen-write-ok");
} catch (e) {
  console.log("frozen-write-error");
}
console.log(frozen.value);

const sealed = new GuardedCounter();
Object.seal(sealed);
sealed.value = 12.5;
try {
  (sealed as any).extra = 99;
  console.log("sealed-extra-ok");
} catch (e) {
  console.log("sealed-extra-error");
}
console.log(JSON.stringify(sealed));

const nonExtensible = new GuardedCounter();
Object.preventExtensions(nonExtensible);
nonExtensible.value = 13.5;
try {
  (nonExtensible as any).extra = 101;
  console.log("prevent-extra-ok");
} catch (e) {
  console.log("prevent-extra-error");
}
console.log(JSON.stringify(nonExtensible));

let accessorSeen = 0;
function readAccessor(): number {
  return 44;
}
function writeAccessor(v: number): void {
  accessorSeen = v;
}

const accessorBox = new GuardedCounter();
Object.defineProperty(accessorBox, "value", {
  get: readAccessor,
  set: writeAccessor,
  enumerable: true,
  configurable: true,
});
console.log(accessorBox.value);
accessorBox.value = 15.5;
console.log(accessorSeen);
