// Edge-case tests for objects, records, computed properties, dynamic access
// Targets bugs like: obj[numericKey] on Record<number,T>, IndexSet union path,
// dynamic property access, object spread, property enumeration

// --- Basic object literal ---
const obj = { a: 1, b: 2, c: 3 };
console.log(obj.a);  // 1
console.log(obj.b);  // 2
console.log(obj.c);  // 3

// --- Computed property access ---
const key = "b";
console.log(obj[key]);  // 2

// --- Dynamic property set ---
const dynObj: Record<string, number> = {};
dynObj["x"] = 10;
dynObj["y"] = 20;
dynObj["z"] = 30;
console.log(dynObj["x"]);  // 10
console.log(dynObj["y"]);  // 20
console.log(dynObj["z"]);  // 30

// --- Object.keys ---
const keysObj = { alpha: 1, beta: 2, gamma: 3 };
const keys = Object.keys(keysObj);
console.log(keys.length);  // 3

// --- Object.values ---
const vals = Object.values(keysObj);
let valSum = 0;
for (let i = 0; i < vals.length; i++) {
    valSum = valSum + vals[i];
}
console.log(valSum);  // 6

// --- Object.entries ---
const entries = Object.entries(keysObj);
console.log(entries.length);  // 3

// --- for...in loop ---
const forInObj: Record<string, number> = { a: 1, b: 2, c: 3 };
const forInKeys: string[] = [];
for (const k in forInObj) {
    forInKeys.push(k);
}
console.log(forInKeys.length);  // 3

// --- Nested objects ---
const nested = {
    level1: {
        level2: {
            value: 42
        }
    }
};
console.log(nested.level1.level2.value);  // 42

// --- Object spread ---
const base = { x: 1, y: 2 };
const extended = { ...base, z: 3 };
console.log(extended.x);  // 1
console.log(extended.y);  // 2
console.log(extended.z);  // 3

// --- Spread overriding properties ---
const override = { ...base, x: 10 };
console.log(override.x);  // 10
console.log(override.y);  // 2

// --- Object destructuring ---
const { a: da, b: db } = { a: 100, b: 200 };
console.log(da);  // 100
console.log(db);  // 200

// --- Destructuring with rename and default ---
const { name: userName = "anonymous", age: userAge = 0 } = { name: "Alice" } as { name?: string; age?: number };
console.log(userName);  // Alice
console.log(userAge);   // 0

// --- Rest in destructuring ---
const { first: fst, ...remaining } = { first: 1, second: 2, third: 3 };
console.log(fst);  // 1
console.log(Object.keys(remaining).length);  // 2

// --- Object as function parameter ---
function describePoint(p: { x: number; y: number }): string {
    return "(" + p.x.toString() + "," + p.y.toString() + ")";
}

console.log(describePoint({ x: 3, y: 4 }));  // (3,4)

// --- Object returned from function ---
function makePoint(x: number, y: number): { x: number; y: number } {
    return { x: x, y: y };
}

const pt = makePoint(5, 6);
console.log(pt.x);  // 5
console.log(pt.y);  // 6

// --- Object with methods ---
const calc = {
    value: 0,
    add(n: number): void {
        this.value = this.value + n;
    },
    get(): number {
        return this.value;
    }
};

calc.add(10);
calc.add(20);
console.log(calc.get());  // 30

// --- Record iteration with accumulation ---
const scores: Record<string, number> = {
    "alice": 95,
    "bob": 87,
    "carol": 92
};

let totalScore = 0;
const scoreKeys = Object.keys(scores);
for (let i = 0; i < scoreKeys.length; i++) {
    totalScore = totalScore + scores[scoreKeys[i]];
}
console.log(totalScore);  // 274

// --- Object with boolean values ---
const flags: Record<string, boolean> = {
    "debug": true,
    "verbose": false,
    "strict": true
};

console.log(flags["debug"]);    // true
console.log(flags["verbose"]);  // false
console.log(flags["strict"]);   // true

// --- Object with array values ---
const groups: Record<string, number[]> = {
    "a": [1, 2, 3],
    "b": [4, 5, 6]
};

console.log(groups["a"].length);  // 3
console.log(groups["b"][1]);      // 5

// --- Object with string values and string keys ---
const translations: Record<string, string> = {
    "hello": "hola",
    "goodbye": "adiós",
    "thanks": "gracias"
};

console.log(translations["hello"]);    // hola
console.log(translations["thanks"]);   // gracias

// --- Checking property existence ---
console.log("hello" in translations);   // true
console.log("missing" in translations); // false

// --- Object equality (reference) ---
const objA = { x: 1 };
const objB = objA;
const objC = { x: 1 };
console.log(objA === objB);  // true
console.log(objA === objC);  // false

// --- Building object incrementally ---
const builder: Record<string, number> = {};
for (let i = 0; i < 5; i++) {
    builder["key" + i.toString()] = i * 10;
}
console.log(builder["key0"]);  // 0
console.log(builder["key3"]);  // 30
console.log(builder["key4"]);  // 40
console.log(Object.keys(builder).length);  // 5

// --- Nested object access in loop ---
const items = [
    { name: "apple", price: 1.5 },
    { name: "banana", price: 0.5 },
    { name: "cherry", price: 2.0 }
];

let totalPrice = 0;
for (let i = 0; i < items.length; i++) {
    totalPrice = totalPrice + items[i].price;
}
console.log(totalPrice);  // 4

// --- Object with optional fields ---
interface Config {
    host: string;
    port?: number;
    debug?: boolean;
}

function applyConfig(cfg: Config): string {
    let result = cfg.host;
    if (cfg.port !== undefined) {
        result = result + ":" + cfg.port.toString();
    }
    if (cfg.debug) {
        result = result + " [debug]";
    }
    return result;
}

console.log(applyConfig({ host: "localhost" }));                      // localhost
console.log(applyConfig({ host: "server", port: 443 }));             // server:443
console.log(applyConfig({ host: "dev", port: 80, debug: true }));    // dev:80 [debug]

// --- Object.assign-like pattern ---
function merge(a: Record<string, number>, b: Record<string, number>): Record<string, number> {
    const result: Record<string, number> = {};
    const aKeys = Object.keys(a);
    for (let i = 0; i < aKeys.length; i++) {
        result[aKeys[i]] = a[aKeys[i]];
    }
    const bKeys = Object.keys(b);
    for (let i = 0; i < bKeys.length; i++) {
        result[bKeys[i]] = b[bKeys[i]];
    }
    return result;
}

const merged = merge({ a: 1, b: 2 }, { b: 3, c: 4 });
console.log(merged["a"]);  // 1
console.log(merged["b"]);  // 3
console.log(merged["c"]);  // 4
