// demonstrates: TypeScript subset shown in docs/src/language/supported-features.md
// docs: docs/src/language/supported-features.md
// platforms: macos, linux, windows

// Each ANCHOR block below is the exact code that the supported-features
// docs page renders inline (via {{#include ... :NAME}}). Snippets that
// stand alone are wrapped in their own functions so top-level identifiers
// don't collide across anchors. The whole file is compiled and run by the
// doc-tests harness — if anything drifts from what the compiler accepts,
// CI fails.

// ANCHOR: primitives
function primitives(): void {
    const n: number = 42;
    const s: string = "hello";
    const b: boolean = true;
    const u: undefined = undefined;
    const nl: null = null;

    console.log(`primitives: n=${n} s=${s} b=${b} u=${u} nl=${nl}`)
}
// ANCHOR_END: primitives

// ANCHOR: variables
function variables(): void {
    let x = 10;
    const y = "immutable";
    var z = true; // var is supported but let/const preferred

    console.log(`variables: x=${x} y=${y} z=${z}`)
}
// ANCHOR_END: variables

// ANCHOR: functions
function functionsDemo(): void {
    function add(a: number, b: number): number {
        return a + b;
    }

    // Optional parameters
    function greet(name: string, greeting: string = "Hello"): string {
        return `${greeting}, ${name}!`;
    }

    // Rest parameters
    function sum(...nums: number[]): number {
        return nums.reduce((a, b) => a + b, 0);
    }

    // Arrow functions
    const double = (x: number) => x * 2;

    console.log(`functions: add=${add(2, 3)} greet=${greet("Perry")} sum=${sum(1, 2, 3)} double=${double(5)}`)
}
// ANCHOR_END: functions

// ANCHOR: classes
class Animal {
    name: string;

    constructor(name: string) {
        this.name = name;
    }

    speak(): string {
        return `${this.name} makes a noise`;
    }
}

class Dog extends Animal {
    speak(): string {
        return `${this.name} barks`;
    }
}

// Static methods
class Counter {
    private static instance: Counter;
    private count: number = 0;

    static getInstance(): Counter {
        if (!Counter.instance) {
            Counter.instance = new Counter();
        }
        return Counter.instance;
    }
}
// ANCHOR_END: classes

function classesDemo(): void {
    const dog = new Dog("Rex")
    console.log(`classes: ${dog.speak()}`)
    const c = Counter.getInstance()
    console.log(`classes: counter ok=${c !== null}`)
}

// ANCHOR: enums
// Numeric enums
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

// String enums
enum Color {
    Red = "RED",
    Green = "GREEN",
    Blue = "BLUE",
}

const dir = Direction.Up;
const color = Color.Red;
// ANCHOR_END: enums

function enumsDemo(): void {
    console.log(`enums: dir=${dir} color=${color}`)
}

// ANCHOR: interfaces
interface User {
    name: string;
    age: number;
    email?: string;
}

type Point = { x: number; y: number };
type StringOrNumber = string | number;
type Callback = (value: number) => void;
// ANCHOR_END: interfaces

function interfacesDemo(): void {
    const u: User = { name: "Alice", age: 30 }
    const p: Point = { x: 1, y: 2 }
    const sn: StringOrNumber = "hi"
    const cb: Callback = (v) => console.log(`cb: ${v}`)
    cb(42)
    console.log(`interfaces: ${u.name} (${p.x},${p.y}) ${sn}`)
}

// ANCHOR: arrays
function arraysDemo(): void {
    const nums: number[] = [1, 2, 3];

    // Array methods
    nums.push(4);
    nums.pop();
    const len = nums.length;
    const doubled = nums.map((x) => x * 2);
    const filtered = nums.filter((x) => x > 2);
    const sum = nums.reduce((acc, x) => acc + x, 0);
    const found = nums.find((x) => x === 3);
    const idx = nums.indexOf(3);
    const joined = nums.join(", ");
    const sliced = nums.slice(1, 3);
    nums.splice(1, 1);
    nums.unshift(0);
    const sorted = nums.sort((a, b) => a - b);
    const reversed = nums.reverse();
    const includes = nums.includes(3);
    const every = nums.every((x) => x > 0);
    const some = nums.some((x) => x > 2);
    nums.forEach((x) => console.log(x));
    const flat = [[1, 2], [3]].flat();
    const concatted = nums.concat([5, 6]);

    // Array.from
    const arr = Array.from([10, 20, 30]);

    // Array.isArray
    const value: any = [1, 2, 3]
    if (Array.isArray(value)) { /* ... */ }

    // for...of iteration
    for (const item of nums) {
        console.log(item);
    }

    console.log(`arrays: len=${len} doubled=${doubled.length} filtered=${filtered.length} sum=${sum} found=${found} idx=${idx} joined=${joined} sliced=${sliced.length} sorted=${sorted.length} reversed=${reversed.length} includes=${includes} every=${every} some=${some} flat=${flat.length} concatted=${concatted.length} arr=${arr.length}`)
}
// ANCHOR_END: arrays

// ANCHOR: objects
function objectsDemo(): void {
    const obj: { name: string; version: number; [k: string]: any } = { name: "Perry", version: 1 };
    obj.name = "Perry 2";

    // Dynamic property access
    const key = "name";
    const val = obj[key];

    // Object.keys, Object.values, Object.entries
    const keys = Object.keys(obj);
    const values = Object.values(obj);
    const entries = Object.entries(obj);

    // Spread
    const copy = { ...obj, extra: true };

    // delete
    delete obj[key];

    console.log(`objects: val=${val} keys=${keys.length} values=${values.length} entries=${entries.length} copy=${copy.extra}`)
}
// ANCHOR_END: objects

// ANCHOR: destructuring
function destructuringDemo(): void {
    // Array destructuring
    const [a, b, ...rest] = [1, 2, 3, 4, 5];

    const user = { name: "Alice", age: 30, email: "a@example.com", id: 1 }
    const obj = { id: 2, role: "admin", level: 5 }

    // Object destructuring
    const { name, age, email = "none" } = user;

    // Rename
    const { name: userName } = user;

    // Rest pattern
    const { id, ...remaining } = obj;

    // Function parameter destructuring
    function process({ name, age }: { name: string; age: number }) {
        console.log(name, age);
    }

    process(user)
    console.log(`destructuring: a=${a} b=${b} rest=${rest.length} name=${name} age=${age} email=${email} userName=${userName} id=${id}`)
}
// ANCHOR_END: destructuring

// ANCHOR: template-literals
function templateLiteralsDemo(): void {
    const name = "world";
    const greeting = `Hello, ${name}!`;
    const multiline = `
  Line 1
  Line 2
`;
    const expr = `Result: ${1 + 2}`;

    console.log(`template-literals: greeting=${greeting} multiline_len=${multiline.length} expr=${expr}`)
}
// ANCHOR_END: template-literals

// ANCHOR: spread-rest
function spreadRestDemo(): void {
    const arr1 = [1, 2]
    const arr2 = [3, 4]
    const defaults = { theme: "light", size: "md" }
    const overrides = { size: "lg" }

    // Array spread
    const combined = [...arr1, ...arr2];

    // Object spread
    const merged = { ...defaults, ...overrides };

    // Rest parameters
    function log(...args: any[]) { /* ... */ }

    log("a", "b", "c")
    console.log(`spread-rest: combined=${combined.length} merged=${merged.size}`)
}
// ANCHOR_END: spread-rest

// ANCHOR: closures
function closuresDemo(): void {
    function makeCounter() {
        let count = 0;
        return {
            increment: () => ++count,
            get: () => count,
        };
    }

    const counter = makeCounter();
    counter.increment();
    console.log(counter.get()); // 1
}
// ANCHOR_END: closures

// ANCHOR: async-await
async function asyncAwaitDemo(): Promise<void> {
    interface Profile { id: number; name: string }

    async function fetchUser(id: number): Promise<Profile> {
        // The docs example uses fetch(...) here; we inline a synthetic
        // result so the snippet compiles and runs hermetically.
        return { id, name: `user-${id}` }
    }

    const data = await fetchUser(1);
    console.log(`async-await: id=${data.id} name=${data.name}`)
}
// ANCHOR_END: async-await

// ANCHOR: promises
async function promisesDemo(): Promise<void> {
    const p = new Promise<number>((resolve, reject) => {
        resolve(42);
    });

    p.then((value) => console.log(value));

    // Promise.all
    const results = await Promise.all([
        Promise.resolve("a"),
        Promise.resolve("b"),
    ]);

    console.log(`promises: results=${results.length}`)
}
// ANCHOR_END: promises

// ANCHOR: generators
function generatorsDemo(): void {
    function* range(start: number, end: number) {
        for (let i = start; i < end; i++) {
            yield i;
        }
    }

    for (const n of range(0, 10)) {
        console.log(n);
    }
}
// ANCHOR_END: generators

// ANCHOR: map-set
function mapSetDemo(): void {
    const map = new Map<string, number>();
    map.set("a", 1);
    map.get("a");
    map.has("a");
    map.delete("a");
    map.size;

    const set = new Set<number>();
    set.add(1);
    set.has(1);
    set.delete(1);
    set.size;

    console.log(`map-set: map_size=${map.size} set_size=${set.size}`)
}
// ANCHOR_END: map-set

// ANCHOR: regex
function regexDemo(): void {
    const re = /hello\s+(\w+)/;
    const match = "hello world".match(re);

    if (re.test("hello perry")) {
        console.log("Matched!");
    }

    const replaced = "hello world".replace(/world/, "perry");

    console.log(`regex: match=${match !== null} replaced=${replaced}`)
}
// ANCHOR_END: regex

// ANCHOR: errors
function errorsDemo(): void {
    try {
        throw new Error("something went wrong");
    } catch (e: any) {
        console.log(e.message);
    } finally {
        console.log("cleanup");
    }
}
// ANCHOR_END: errors

// ANCHOR: json
function jsonDemo(): void {
    const obj = JSON.parse('{"key": "value"}');
    const str = JSON.stringify(obj);
    const pretty = JSON.stringify(obj, null, 2);

    console.log(`json: str_len=${str.length} pretty_len=${pretty.length}`)
}
// ANCHOR_END: json

// ANCHOR: typeof-instanceof
function typeofInstanceofDemo(): void {
    const x: any = "hello"
    if (typeof x === "string") {
        console.log(x.length);
    }

    const obj: any = new Dog("Rex")
    if (obj instanceof Dog) {
        obj.speak();
    }
}
// ANCHOR_END: typeof-instanceof

// ANCHOR: bigint
function bigintDemo(): void {
    const big = BigInt(9007199254740991);
    const result = big + BigInt(1);

    // Bitwise operations
    const and = big & BigInt(0xFF);
    const or = big | BigInt(0xFF);
    const xor = big ^ BigInt(0xFF);
    const shl = big << BigInt(2);
    const shr = big >> BigInt(2);
    const not = ~big;

    console.log(`bigint: result_ok=${result !== null} and_ok=${and !== null} or_ok=${or !== null}`)
}
// ANCHOR_END: bigint

// ANCHOR: symbol
function symbolDemo(): void {
    const local = Symbol("local");
    const shared = Symbol.for("shared");
    const sharedAgain = Symbol.for("shared");
    const obj: Record<string | symbol, any> = { name: "Perry" };

    obj[local] = "symbol value";

    console.log(`symbol: type=${typeof local} desc=${local.description} shared=${shared === sharedAgain} own=${Object.getOwnPropertySymbols(obj).length}`)
}
// ANCHOR_END: symbol

// ANCHOR: string-methods
function stringMethodsDemo(): void {
    const s = "Hello, World!";
    s.length;
    s.toUpperCase();
    s.toLowerCase();
    s.trim();
    s.split(", ");
    s.includes("World");
    s.startsWith("Hello");
    s.endsWith("!");
    s.indexOf("World");
    s.slice(0, 5);
    s.substring(0, 5);
    s.replace("World", "Perry");
    s.repeat(3);
    s.charAt(0);
    s.padStart(20);
    s.padEnd(20);

    console.log(`string-methods: ${s.toUpperCase()}`)
}
// ANCHOR_END: string-methods

// ANCHOR: math
function mathDemo(): void {
    Math.floor(3.7);
    Math.ceil(3.2);
    Math.round(3.5);
    Math.abs(-5);
    Math.max(1, 2, 3);
    Math.min(1, 2, 3);
    Math.sqrt(16);
    Math.pow(2, 10);
    Math.random();
    Math.PI;
    Math.E;
    Math.log(10);
    Math.sin(0);
    Math.cos(0);

    console.log(`math: floor=${Math.floor(3.7)} sqrt=${Math.sqrt(16)}`)
}
// ANCHOR_END: math

// ANCHOR: date
function dateDemo(): void {
    const now = Date.now();
    const d = new Date();
    d.getTime();
    d.toISOString();

    console.log(`date: now_positive=${now > 0}`)
}
// ANCHOR_END: date

// ANCHOR: console
function consoleDemo(): void {
    console.log("message");
    console.error("error");
    console.warn("warning");
    console.time("label");
    console.timeEnd("label");
}
// ANCHOR_END: console

// ANCHOR: gc
function gcDemo(): void {
    gc(); // Explicit garbage collection
}
// ANCHOR_END: gc

// driver
primitives()
variables()
functionsDemo()
classesDemo()
enumsDemo()
interfacesDemo()
arraysDemo()
objectsDemo()
destructuringDemo()
templateLiteralsDemo()
spreadRestDemo()
closuresDemo()

async function main(): Promise<void> {
    await asyncAwaitDemo()
    await promisesDemo()
}
main()

generatorsDemo()
mapSetDemo()
regexDemo()
errorsDemo()
jsonDemo()
typeofInstanceofDemo()
bigintDemo()
symbolDemo()
stringMethodsDemo()
mathDemo()
dateDemo()
consoleDemo()
gcDemo()
