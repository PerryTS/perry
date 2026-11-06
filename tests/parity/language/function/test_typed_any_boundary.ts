// Test type boundaries: typed ↔ any function calls and assignments
// Critical for static typing migration: ensures values survive boxing/unboxing at type boundaries

function getAnyNumber(): any { return 42; }
function getAnyString(): any { return "hello"; }
function getAnyBool(): any { return true; }
function getAnyArray(): any { return [1, 2, 3]; }
function getAnyObject(): any { return { x: 10 }; }

function acceptAny(x: any): string { return String(x); }
function addNumbers(a: number, b: number): number { return a + b; }
function concatStrings(a: string, b: string): string { return a + b; }
function negate(b: boolean): boolean { return !b; }

// === Typed variable assigned from any-returning function ===
const n: number = getAnyNumber();
console.log(n);          // 42
console.log(n + 8);      // 50
console.log(addNumbers(n, 3)); // 45

const s: string = getAnyString();
console.log(s);          // hello
console.log(s + " world"); // hello world
console.log(concatStrings(s, "!")); // hello!

const b: boolean = getAnyBool();
console.log(b);          // true

// === Typed values passed to any-accepting function ===
console.log(acceptAny(42));      // 42
console.log(acceptAny("test"));  // test
console.log(acceptAny(true));    // true
console.log(acceptAny(false));   // false

// === Any value used in typed arithmetic ===
const anyVal: any = 10;
const typedResult: number = anyVal + 5;
console.log(typedResult); // 15

// === Any value in conditional ===
const anyBool: any = false;
if (anyBool) {
  console.log("WRONG");
} else {
  console.log("correct falsy"); // correct falsy
}

const anyTruthy: any = "nonempty";
if (anyTruthy) {
  console.log("correct truthy"); // correct truthy
} else {
  console.log("WRONG");
}

// === Chain: typed → any → typed ===
function typedToAny(x: number): any { return x * 2; }
function anyToTyped(x: any): number { return x + 1; }
const chained: number = anyToTyped(typedToAny(5));
console.log(chained); // 11

// === Class instance through any boundary ===
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  sum(): number { return this.x + this.y; }
}

function getAnyPoint(): any { return new Point(3, 4); }
const p: Point = getAnyPoint();
console.log(p.x);     // 3
console.log(p.sum());  // 7

// === Array through any boundary ===
function getAnyArr(): any { return [10, 20, 30]; }
const arr: number[] = getAnyArr();
console.log(arr.length); // 3
console.log(arr[1]);     // 20
