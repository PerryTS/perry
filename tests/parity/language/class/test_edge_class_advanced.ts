// Advanced class edge cases: private fields, abstract-like patterns,
// class expressions, mixins, method binding, this context,
// class with closures, class with generics, class with static

// --- Private fields ---
class BankAccount {
    private _balance: number;
    private _owner: string;

    constructor(owner: string, initial: number) {
        this._owner = owner;
        this._balance = initial;
    }

    deposit(amount: number): void {
        this._balance = this._balance + amount;
    }

    withdraw(amount: number): boolean {
        if (amount > this._balance) return false;
        this._balance = this._balance - amount;
        return true;
    }

    getBalance(): number {
        return this._balance;
    }

    toString(): string {
        return this._owner + ": $" + this._balance.toString();
    }
}

const account = new BankAccount("Alice", 100);
account.deposit(50);
console.log(account.getBalance());  // 150
console.log(account.withdraw(30));  // true
console.log(account.getBalance());  // 120
console.log(account.withdraw(200)); // false
console.log(account.getBalance());  // 120
console.log(account.toString());    // Alice: $120

// --- Static factory method ---
class Color2 {
    r: number;
    g: number;
    b: number;

    constructor(r: number, g: number, b: number) {
        this.r = r;
        this.g = g;
        this.b = b;
    }

    static red(): Color2 { return new Color2(255, 0, 0); }
    static green(): Color2 { return new Color2(0, 255, 0); }
    static blue(): Color2 { return new Color2(0, 0, 255); }

    toString(): string {
        return "rgb(" + this.r.toString() + "," + this.g.toString() + "," + this.b.toString() + ")";
    }
}

console.log(Color2.red().toString());    // rgb(255,0,0)
console.log(Color2.green().toString());  // rgb(0,255,0)
console.log(Color2.blue().toString());   // rgb(0,0,255)

// --- Class with generic type ---
class LinkedList<T> {
    items: T[];

    constructor() {
        this.items = [];
    }

    add(item: T): void {
        this.items.push(item);
    }

    get(index: number): T {
        return this.items[index];
    }

    size(): number {
        return this.items.length;
    }

    toArray(): T[] {
        return [...this.items];
    }
}

const numList = new LinkedList<number>();
numList.add(10);
numList.add(20);
numList.add(30);
console.log(numList.get(0));  // 10
console.log(numList.get(2));  // 30
console.log(numList.size());  // 3

const strList = new LinkedList<string>();
strList.add("a");
strList.add("b");
console.log(strList.toArray().join(","));  // a,b

// --- Deep inheritance with method override at each level ---
class A {
    value(): number { return 1; }
    label(): string { return "A"; }
}

class B extends A {
    value(): number { return super.value() + 10; }
    label(): string { return "B>" + super.label(); }
}

class C extends B {
    value(): number { return super.value() + 100; }
    label(): string { return "C>" + super.label(); }
}

class D extends C {
    value(): number { return super.value() + 1000; }
    label(): string { return "D>" + super.label(); }
}

const d = new D();
console.log(d.value());  // 1111
console.log(d.label());  // D>C>B>A

// --- Class with closure-returning method ---
class Multiplier {
    factor: number;

    constructor(factor: number) {
        this.factor = factor;
    }

    getMultiplier(): (x: number) => number {
        const f = this.factor;
        return (x: number) => x * f;
    }
}

const times3 = new Multiplier(3).getMultiplier();
const times5 = new Multiplier(5).getMultiplier();
console.log(times3(7));   // 21
console.log(times5(7));   // 35

// --- Class implementing multiple interface patterns ---
class StringBuffer {
    private parts: string[];

    constructor() {
        this.parts = [];
    }

    append(s: string): StringBuffer {
        this.parts.push(s);
        return this;
    }

    prepend(s: string): StringBuffer {
        this.parts = [s, ...this.parts];
        return this;
    }

    toString(): string {
        return this.parts.join("");
    }

    length(): number {
        let total = 0;
        for (let i = 0; i < this.parts.length; i++) {
            total = total + this.parts[i].length;
        }
        return total;
    }
}

const sb = new StringBuffer();
sb.append("Hello").append(", ").append("World").prepend(">>> ");
console.log(sb.toString());  // >>> Hello, World
console.log(sb.length());    // 18

// --- Class with array field mutation ---
class TodoList {
    items: string[];

    constructor() {
        this.items = [];
    }

    add(item: string): void {
        this.items.push(item);
    }

    remove(index: number): void {
        this.items.splice(index, 1);
    }

    getAll(): string[] {
        return this.items;
    }
}

const todos = new TodoList();
todos.add("Buy groceries");
todos.add("Clean house");
todos.add("Write code");
console.log(todos.getAll().length);  // 3
todos.remove(1);
console.log(todos.getAll().length);  // 2
console.log(todos.getAll().join(", "));  // Buy groceries, Write code

// --- Class with computed values ---
class Stats {
    values: number[];

    constructor(values: number[]) {
        this.values = values;
    }

    sum(): number {
        let s = 0;
        for (let i = 0; i < this.values.length; i++) {
            s = s + this.values[i];
        }
        return s;
    }

    mean(): number {
        return this.sum() / this.values.length;
    }

    min(): number {
        let m = this.values[0];
        for (let i = 1; i < this.values.length; i++) {
            if (this.values[i] < m) m = this.values[i];
        }
        return m;
    }

    max(): number {
        let m = this.values[0];
        for (let i = 1; i < this.values.length; i++) {
            if (this.values[i] > m) m = this.values[i];
        }
        return m;
    }
}

const stats = new Stats([10, 20, 30, 40, 50]);
console.log(stats.sum());   // 150
console.log(stats.mean());  // 30
console.log(stats.min());   // 10
console.log(stats.max());   // 50

// --- Polymorphism with array of base class ---
class Shape3 {
    area(): number { return 0; }
    perimeter(): number { return 0; }
    describe(): string {
        return "area=" + this.area().toString() + " perimeter=" + this.perimeter().toString();
    }
}

class Circle3 extends Shape3 {
    r: number;
    constructor(r: number) { super(); this.r = r; }
    area(): number { return Math.PI * this.r * this.r; }
    perimeter(): number { return 2 * Math.PI * this.r; }
}

class Rect3 extends Shape3 {
    w: number;
    h: number;
    constructor(w: number, h: number) { super(); this.w = w; this.h = h; }
    area(): number { return this.w * this.h; }
    perimeter(): number { return 2 * (this.w + this.h); }
}

const shapes3: Shape3[] = [new Circle3(5), new Rect3(3, 4)];
for (let i = 0; i < shapes3.length; i++) {
    console.log(Math.round(shapes3[i].area()));
}
// 79
// 12

// --- Constructor calling methods ---
class Initializer {
    data: string;

    constructor(raw: string) {
        this.data = this.process(raw);
    }

    process(s: string): string {
        return s.trim().toUpperCase();
    }
}

const init = new Initializer("  hello world  ");
console.log(init.data);  // HELLO WORLD

// --- Class with optional constructor params ---
class HttpRequest {
    method: string;
    url: string;
    body: string;

    constructor(url: string, method: string = "GET", body: string = "") {
        this.url = url;
        this.method = method;
        this.body = body;
    }

    toString(): string {
        return this.method + " " + this.url + (this.body ? " [" + this.body + "]" : "");
    }
}

console.log(new HttpRequest("/api/users").toString());              // GET /api/users
console.log(new HttpRequest("/api/users", "POST", "{}").toString()); // POST /api/users [{}]

// --- Class field access through function ---
class Box2 {
    value: number;
    constructor(v: number) { this.value = v; }
}

function getBoxValue(b: Box2): number {
    return b.value;
}

function setBoxValue(b: Box2, v: number): void {
    b.value = v;
}

const box = new Box2(10);
console.log(getBoxValue(box));  // 10
setBoxValue(box, 99);
console.log(getBoxValue(box));  // 99

// --- Array of class instances with method calls ---
class Counter2 {
    count: number;
    constructor() { this.count = 0; }
    inc(): void { this.count++; }
    get(): number { return this.count; }
}

const counters = [new Counter2(), new Counter2(), new Counter2()];
counters[0].inc();
counters[0].inc();
counters[1].inc();
counters[2].inc();
counters[2].inc();
counters[2].inc();

const counts = counters.map((c: Counter2) => c.get());
console.log(counts.join(","));  // 2,1,3
