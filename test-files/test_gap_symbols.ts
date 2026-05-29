// Test: Symbol features (Perry gap analysis)
// These features are NOT yet supported by Perry — this file documents the target behavior.
// Run: node --experimental-strip-types test-files/test_gap_symbols.ts

// --- Symbol() creation ---
const s1 = Symbol();
const s2 = Symbol();
console.log(s1 !== s2);         // true
console.log(typeof s1);         // symbol

// --- Symbol('description') with description ---
const named = Symbol("mySymbol");
console.log(named.description); // mySymbol
console.log(named.toString());  // Symbol(mySymbol)

// --- Symbol.for('key') global registry ---
const global1 = Symbol.for("shared");
const global2 = Symbol.for("shared");
console.log(global1 === global2); // true

// --- Symbol.keyFor(sym) ---
const registered = Symbol.for("registered");
console.log(Symbol.keyFor(registered));  // registered
const local = Symbol("local");
console.log(Symbol.keyFor(local));       // undefined

// --- Symbol as object property key ---
const symKey = Symbol("key");
const obj: Record<symbol | string, any> = {};
obj[symKey] = "symbol value";
obj["str"] = "string value";
console.log(obj[symKey]); // symbol value
console.log(obj["str"]);  // string value

// Using computed property in literal
const symProp = Symbol("prop");
const obj2 = {
  [symProp]: 42,
  normal: "hello",
};
console.log(obj2[symProp]); // 42
console.log(obj2.normal);   // hello

// --- typeof sym === 'symbol' ---
const checkSym = Symbol("check");
console.log(typeof checkSym === "symbol"); // true
console.log(typeof checkSym === "string"); // false
console.log(typeof "hello" === "symbol");  // false

// --- Symbol.iterator — make a class iterable ---
class Fibonacci {
  limit: number;
  constructor(limit: number) {
    this.limit = limit;
  }

  *[Symbol.iterator](): Generator<number> {
    let a = 0;
    let b = 1;
    let count = 0;
    while (count < this.limit) {
      yield a;
      [a, b] = [b, a + b];
      count++;
    }
  }
}

const fibs: number[] = [];
for (const f of new Fibonacci(7)) {
  fibs.push(f);
}
console.log(fibs.join(",")); // 0,1,1,2,3,5,8

// --- Symbol.toPrimitive — customize type coercion ---
const currency = {
  value: 100,
  currency: "USD",
  [Symbol.toPrimitive](hint: string): string | number {
    if (hint === "number") return this.value;
    if (hint === "string") return `${this.value} ${this.currency}`;
    return this.value; // default
  },
};

console.log(+currency);           // 100
console.log(`${currency}`);       // 100 USD
console.log(currency + 0);        // 100

// --- Symbol.toPrimitive on a CLASS instance (#2374) ---
// The method lives on the prototype, not as an own symbol property, so the
// coercion paths must resolve it through the class vtable.
class Temperature {
  celsius: number;
  constructor(celsius: number) {
    this.celsius = celsius;
  }
  [Symbol.toPrimitive](hint: string): string | number {
    if (hint === "number") return this.celsius;
    if (hint === "string") return `${this.celsius}°C`;
    return this.celsius; // default
  }
}

const t = new Temperature(42);
console.log(+t);            // 42
console.log(`${t}`);        // 42°C
console.log(String(t));     // 42°C
console.log(t + 0);         // 42 (default hint → number)

// --- Symbol.hasInstance — customize instanceof ---
class EvenChecker {
  static [Symbol.hasInstance](value: any): boolean {
    return typeof value === "number" && value % 2 === 0;
  }
}

console.log(4 instanceof EvenChecker);  // true
console.log(3 instanceof EvenChecker);  // false

// --- Symbol.toStringTag — customize Object.prototype.toString ---
class MyCollection {
  get [Symbol.toStringTag](): string {
    return "MyCollection";
  }
}

const col = new MyCollection();
console.log(Object.prototype.toString.call(col)); // [object MyCollection]

// --- Symbols in for...in (should be excluded) ---
const symHidden = Symbol("hidden");
const testObj: Record<string | symbol, any> = {
  visible: 1,
  also: 2,
  [symHidden]: 3,
};

const forInKeys: string[] = [];
for (const key in testObj) {
  forInKeys.push(key);
}
console.log(forInKeys.join(","));           // visible,also
console.log(forInKeys.includes("hidden"));  // false

// --- Object.getOwnPropertySymbols() ---
const symA = Symbol("a");
const symB = Symbol("b");
const symbolObj = {
  [symA]: 1,
  [symB]: 2,
  regular: 3,
};

const ownSymbols = Object.getOwnPropertySymbols(symbolObj);
console.log(ownSymbols.length);                   // 2
console.log(symbolObj[ownSymbols[0] as symbol]);  // 1
console.log(symbolObj[ownSymbols[1] as symbol]);  // 2

// Verify regular keys don't appear in symbol list
console.log(Object.keys(symbolObj).join(",")); // regular

console.log("ALL SYMBOL TESTS PASSED");
