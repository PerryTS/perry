// Edge-case tests for classes, inheritance, and OOP patterns
// Tests: deep inheritance, method overriding, super calls, static members,
//        getters/setters, instanceof, constructor chaining

// --- Basic class with constructor and methods ---
class Animal {
    name: string;
    sound: string;

    constructor(name: string, sound: string) {
        this.name = name;
        this.sound = sound;
    }

    speak(): string {
        return this.name + " says " + this.sound;
    }

    toString(): string {
        return "[Animal: " + this.name + "]";
    }
}

const dog = new Animal("Dog", "Woof");
console.log(dog.speak());     // Dog says Woof
console.log(dog.toString());  // [Animal: Dog]

// --- Single inheritance with super ---
class Dog extends Animal {
    breed: string;

    constructor(breed: string) {
        super("Dog", "Woof");
        this.breed = breed;
    }

    speak(): string {
        return super.speak() + " (" + this.breed + ")";
    }
}

const lab = new Dog("Labrador");
console.log(lab.speak());  // Dog says Woof (Labrador)
console.log(lab.name);     // Dog
console.log(lab.breed);    // Labrador

// --- Deep inheritance chain (3+ levels) ---
class Shape {
    kind: string;
    constructor(kind: string) {
        this.kind = kind;
    }
    area(): number {
        return 0;
    }
    describe(): string {
        return this.kind + " with area " + this.area().toString();
    }
}

class Rectangle extends Shape {
    width: number;
    height: number;
    constructor(w: number, h: number) {
        super("Rectangle");
        this.width = w;
        this.height = h;
    }
    area(): number {
        return this.width * this.height;
    }
}

class Square extends Rectangle {
    constructor(side: number) {
        super(side, side);
        this.kind = "Square";
    }
}

const rect = new Rectangle(3, 4);
const sq = new Square(5);
console.log(rect.area());      // 12
console.log(rect.describe());  // Rectangle with area 12
console.log(sq.area());        // 25
console.log(sq.describe());    // Square with area 25
console.log(sq.width);         // 5

// --- instanceof checks ---
console.log(sq instanceof Square);     // true
console.log(sq instanceof Rectangle);  // true
console.log(sq instanceof Shape);      // true
console.log(rect instanceof Square);   // false

// --- Static members ---
class Counter {
    static count: number = 0;
    id: number;

    constructor() {
        Counter.count = Counter.count + 1;
        this.id = Counter.count;
    }

    static getCount(): number {
        return Counter.count;
    }
}

const c1 = new Counter();
const c2 = new Counter();
const c3 = new Counter();
console.log(c1.id);              // 1
console.log(c2.id);              // 2
console.log(c3.id);              // 3
console.log(Counter.getCount()); // 3

// --- Getters and setters ---
class Temperature {
    private _celsius: number;

    constructor(celsius: number) {
        this._celsius = celsius;
    }

    get celsius(): number {
        return this._celsius;
    }

    set celsius(value: number) {
        this._celsius = value;
    }

    get fahrenheit(): number {
        return this._celsius * 9 / 5 + 32;
    }

    set fahrenheit(value: number) {
        this._celsius = (value - 32) * 5 / 9;
    }
}

const temp = new Temperature(100);
console.log(temp.celsius);     // 100
console.log(temp.fahrenheit);  // 212
temp.fahrenheit = 32;
console.log(temp.celsius);     // 0

// --- Method overriding with different return patterns ---
class Base {
    value(): number { return 1; }
    label(): string { return "Base"; }
}

class Derived extends Base {
    value(): number { return super.value() + 10; }
    label(): string { return "Derived"; }
}

class DerivedAgain extends Derived {
    value(): number { return super.value() + 100; }
    label(): string { return super.label() + "+"; }
}

const da = new DerivedAgain();
console.log(da.value());  // 111
console.log(da.label());  // Derived+

// --- Class with array and object fields ---
class DataStore {
    items: number[];
    metadata: Record<string, string>;

    constructor() {
        this.items = [];
        this.metadata = {};
    }

    addItem(n: number): void {
        this.items.push(n);
    }

    setMeta(key: string, value: string): void {
        this.metadata[key] = value;
    }

    getTotal(): number {
        let sum = 0;
        for (let i = 0; i < this.items.length; i++) {
            sum = sum + this.items[i];
        }
        return sum;
    }
}

const ds = new DataStore();
ds.addItem(10);
ds.addItem(20);
ds.addItem(30);
ds.setMeta("owner", "test");
console.log(ds.getTotal());           // 60
console.log(ds.items.length);         // 3
console.log(ds.metadata["owner"]);    // test

// --- Class methods returning 'this' for chaining ---
class Builder {
    parts: string[];

    constructor() {
        this.parts = [];
    }

    add(part: string): Builder {
        this.parts.push(part);
        return this;
    }

    build(): string {
        return this.parts.join(", ");
    }
}

const result = new Builder().add("a").add("b").add("c").build();
console.log(result);  // a, b, c

// --- Constructor with default parameters ---
class Config {
    host: string;
    port: number;
    debug: boolean;

    constructor(host: string = "localhost", port: number = 8080, debug: boolean = false) {
        this.host = host;
        this.port = port;
        this.debug = debug;
    }

    toString(): string {
        return this.host + ":" + this.port.toString() + (this.debug ? " [debug]" : "");
    }
}

console.log(new Config().toString());                     // localhost:8080
console.log(new Config("example.com", 443).toString());   // example.com:443
console.log(new Config("x", 80, true).toString());        // x:80 [debug]

// --- Class with methods that use closures ---
class MyEmitter {
    listeners: Array<(data: string) => void>;

    constructor() {
        this.listeners = [];
    }

    on(fn: (data: string) => void): void {
        this.listeners.push(fn);
    }

    emit(data: string): void {
        for (let i = 0; i < this.listeners.length; i++) {
            this.listeners[i](data);
        }
    }
}

const emitter = new MyEmitter();
const log: string[] = [];
emitter.on((data: string) => { log.push("A:" + data); });
emitter.on((data: string) => { log.push("B:" + data); });
emitter.emit("hello");
console.log(log.join(", "));  // A:hello, B:hello

// --- Polymorphic method calls through base type ---
class Printer {
    format(s: string): string {
        return s;
    }
}

class UpperPrinter extends Printer {
    format(s: string): string {
        return s.toUpperCase();
    }
}

class QuotePrinter extends Printer {
    format(s: string): string {
        return '"' + s + '"';
    }
}

function printAll(printers: Printer[], text: string): void {
    for (let i = 0; i < printers.length; i++) {
        console.log(printers[i].format(text));
    }
}

printAll([new Printer(), new UpperPrinter(), new QuotePrinter()], "hello");
// hello
// HELLO
// "hello"

// --- Class field initialization order ---
class FieldOrder {
    a: number;
    b: string;
    c: boolean;

    constructor() {
        this.a = 42;
        this.b = "test";
        this.c = true;
    }
}

const fo = new FieldOrder();
console.log(fo.a);  // 42
console.log(fo.b);  // test
console.log(fo.c);  // true

// --- Inherited method accessing subclass fields ---
class Vehicle {
    speed: number;

    constructor(speed: number) {
        this.speed = speed;
    }

    info(): string {
        return "speed=" + this.speed.toString();
    }
}

class Car extends Vehicle {
    doors: number;

    constructor(speed: number, doors: number) {
        super(speed);
        this.doors = doors;
    }
}

const car = new Car(120, 4);
console.log(car.info());   // speed=120
console.log(car.doors);    // 4
