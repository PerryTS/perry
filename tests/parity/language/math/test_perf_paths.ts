// Test: Performance-critical paths exercised by optimization targets
// Covers: string comparison, typeof, array access, string/array length,
// direct field access on known class instances, object field lookup

// === 1. String comparison (memcmp target) ===
let a: string = "hello world";
let b: string = "hello world";
let c: string = "hello worlx";
console.log(a === b);  // true
console.log(a === c);  // false
console.log("" === ""); // true

// === 2. typeof (interning target) ===
let num: number = 42;
let str: string = "test";
let flag: boolean = true;
let obj = { x: 1 };
let arr: number[] = [1, 2, 3];
console.log(typeof num);    // number
console.log(typeof str);    // string
console.log(typeof flag);   // boolean
console.log(typeof obj);    // object
console.log(typeof undefined); // undefined
console.log(typeof null);   // object

// === 3. Array access patterns (polymorphic check target) ===
let nums: number[] = [10, 20, 30, 40, 50];
console.log(nums[0]);  // 10
console.log(nums[4]);  // 50
console.log(nums.length); // 5

// array iteration with index
let sum: number = 0;
for (let i = 0; i < nums.length; i++) {
  sum += nums[i];
}
console.log(sum); // 150

// === 4. String/array length (inline target) ===
let greeting: string = "hello";
console.log(greeting.length); // 5
let items: number[] = [1, 2, 3, 4];
console.log(items.length); // 4

// === 5. Direct field access on known class instance ===
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  distanceTo(other: Point): number {
    let dx = this.x - other.x;
    let dy = this.y - other.y;
    return Math.sqrt(dx * dx + dy * dy);
  }
}

let p1 = new Point(0, 0);
let p2 = new Point(3, 4);
console.log(p1.x);  // 0
console.log(p2.y);  // 4
console.log(p1.distanceTo(p2)); // 5

// Access fields on a variable of known class type
function getX(p: Point): number {
  return p.x;
}
console.log(getX(p2)); // 3

// === 6. Object field lookup (hash map target) ===
let config = { host: "localhost", port: 8080, debug: true };
console.log(config.host); // localhost
console.log(config.port); // 8080

// === 7. Mixed operations ===
class Person {
  name: string;
  age: number;
  constructor(name: string, age: number) {
    this.name = name;
    this.age = age;
  }
  greet(): string {
    return "Hi, " + this.name;
  }
}

let people: Person[] = [new Person("Alice", 30), new Person("Bob", 25)];
for (let i = 0; i < people.length; i++) {
  console.log(people[i].greet());
}
// Hi, Alice
// Hi, Bob

console.log("perf_paths_ok");
