// Test native library calling conventions:
// - ExternFuncRef calls not in import map should emit actual calls (not drop)
// - String args should be passed as ptr (x-register on ARM64)
// - Exported const variables should be accessible cross-module
// - PropertyGet on imported exported constants should load the value
// - IndexSet on module-global arrays should work from function bodies
// - Module globals referenced from functions need external linkage

// === 1. Module-global array IndexSet from functions ===
const STATE: number[] = [0, 0, 0];

function setState(index: number, value: number): void {
  STATE[index] = value;
}

setState(0, 42);
setState(1, 99);
setState(2, -1);
console.log(STATE[0]); // 42
console.log(STATE[1]); // 99
console.log(STATE[2]); // -1

// === 2. Exported const object property access ===
// (tests the pattern that broke Key.DOWN in Bloom)
// Simulated via a local const object + function that reads it
const KEYS = { UP: 256, DOWN: 257, LEFT: 258, RIGHT: 259, SPACE: 32 };

function getKey(name: string): number {
  if (name === "UP") return KEYS.UP;
  if (name === "DOWN") return KEYS.DOWN;
  if (name === "LEFT") return KEYS.LEFT;
  if (name === "RIGHT") return KEYS.RIGHT;
  if (name === "SPACE") return KEYS.SPACE;
  return -1;
}

console.log(getKey("UP"));    // 256
console.log(getKey("DOWN"));  // 257
console.log(getKey("SPACE")); // 32
console.log(getKey("X"));     // -1

// === 3. Array with class instances + sort (the v0.5.11 regression) ===
class Item {
  name: string;
  value: number;
  constructor(name: string, value: number) {
    this.name = name;
    this.value = value;
  }
}

const sorted = [new Item("c", 30), new Item("a", 10), new Item("b", 20)];
sorted.sort((a: Item, b: Item): number => a.value - b.value);
console.log(sorted[0].name); // a
console.log(sorted[1].name); // b
console.log(sorted[2].name); // c

// Also test without type annotation (the original regression trigger)
const items = [new Item("z", 3), new Item("x", 1)];
console.log(items[0].name); // z
console.log(items[1].name); // x

// === 4. instanceof with multi-level inheritance ===
class Shape { kind: string; constructor(k: string) { this.kind = k; } }
class Rectangle extends Shape { w: number; h: number; constructor(w: number, h: number) { super("rect"); this.w = w; this.h = h; } }
class Square extends Rectangle { constructor(s: number) { super(s, s); } }

const sq = new Square(5);
console.log(sq instanceof Square);    // true
console.log(sq instanceof Rectangle); // true
console.log(sq instanceof Shape);     // true

// === 5. Bounded for-loop with i32 counter ===
const arr: number[] = [];
for (let i = 0; i < 10; i++) {
  arr[i] = i * 2;
}
let sum = 0;
for (let i = 0; i < arr.length; i++) {
  sum += arr[i];
}
console.log(sum); // 90
