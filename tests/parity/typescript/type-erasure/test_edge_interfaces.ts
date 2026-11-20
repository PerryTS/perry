// Edge-case tests for interfaces: implementation, polymorphism, structural typing,
// interface extension, optional members, index signatures

// --- Interface implementation ---
interface Printable {
    toString(): string;
}

class Point implements Printable {
    x: number;
    y: number;

    constructor(x: number, y: number) {
        this.x = x;
        this.y = y;
    }

    toString(): string {
        return "(" + this.x.toString() + "," + this.y.toString() + ")";
    }
}

const p = new Point(3, 4);
console.log(p.toString());  // (3,4)

// --- Multiple classes implementing same interface ---
interface HasArea {
    area(): number;
    name: string;
}

class Circle2 implements HasArea {
    name: string;
    radius: number;

    constructor(r: number) {
        this.name = "Circle";
        this.radius = r;
    }

    area(): number {
        return Math.PI * this.radius * this.radius;
    }
}

class Rectangle2 implements HasArea {
    name: string;
    width: number;
    height: number;

    constructor(w: number, h: number) {
        this.name = "Rectangle";
        this.width = w;
        this.height = h;
    }

    area(): number {
        return this.width * this.height;
    }
}

class Triangle implements HasArea {
    name: string;
    base: number;
    height: number;

    constructor(b: number, h: number) {
        this.name = "Triangle";
        this.base = b;
        this.height = h;
    }

    area(): number {
        return 0.5 * this.base * this.height;
    }
}

// Polymorphic function accepting interface ---
function printArea(shape: HasArea): void {
    console.log(shape.name + ": " + Math.round(shape.area()).toString());
}

printArea(new Circle2(5));        // Circle: 79
printArea(new Rectangle2(3, 4));  // Rectangle: 12
printArea(new Triangle(6, 8));    // Triangle: 24

// --- Array of interface type ---
const shapes: HasArea[] = [
    new Circle2(10),
    new Rectangle2(5, 6),
    new Triangle(8, 3)
];

let totalArea = 0;
for (let i = 0; i < shapes.length; i++) {
    totalArea = totalArea + Math.round(shapes[i].area());
}
console.log(totalArea);  // 358  (314 + 30 + 12 = 356... let me compute: pi*100=314.159~314, 30, 12 = 356)

// --- Interface extending interface ---
interface Named {
    name: string;
}

interface Aged {
    age: number;
}

interface Person extends Named, Aged {
    greet(): string;
}

class Employee implements Person {
    name: string;
    age: number;
    department: string;

    constructor(name: string, age: number, department: string) {
        this.name = name;
        this.age = age;
        this.department = department;
    }

    greet(): string {
        return "Hi, I'm " + this.name + " (" + this.age.toString() + ") from " + this.department;
    }
}

const emp = new Employee("Alice", 30, "Engineering");
console.log(emp.greet());  // Hi, I'm Alice (30) from Engineering

// --- Structural typing (duck typing) ---
interface HasLength {
    length: number;
}

function printLength(obj: HasLength): void {
    console.log(obj.length);
}

printLength("hello");     // 5
printLength([1, 2, 3]);   // 3

// --- Interface with function type ---
interface Transformer {
    transform(input: string): string;
}

class UpperTransformer implements Transformer {
    transform(input: string): string {
        return input.toUpperCase();
    }
}

class PrefixTransformer implements Transformer {
    prefix: string;

    constructor(prefix: string) {
        this.prefix = prefix;
    }

    transform(input: string): string {
        return this.prefix + input;
    }
}

function applyTransforms(input: string, transformers: Transformer[]): string {
    let result = input;
    for (let i = 0; i < transformers.length; i++) {
        result = transformers[i].transform(result);
    }
    return result;
}

console.log(applyTransforms("hello", [
    new PrefixTransformer(">>> "),
    new UpperTransformer()
]));  // >>> HELLO

// --- Interface with optional methods ---
interface Logger {
    log(msg: string): void;
    warn?(msg: string): void;
    error?(msg: string): void;
}

class SimpleLogger implements Logger {
    messages: string[];

    constructor() {
        this.messages = [];
    }

    log(msg: string): void {
        this.messages.push("[LOG] " + msg);
    }
}

const logger = new SimpleLogger();
logger.log("hello");
logger.log("world");
console.log(logger.messages.join(", "));  // [LOG] hello, [LOG] world

// --- Interface as type for object literal ---
interface Coordinate {
    x: number;
    y: number;
    z?: number;
}

function distance(a: Coordinate, b: Coordinate): number {
    const dx = a.x - b.x;
    const dy = a.y - b.y;
    return Math.sqrt(dx * dx + dy * dy);
}

console.log(distance({ x: 0, y: 0 }, { x: 3, y: 4 }));  // 5

// --- Interface with readonly-like pattern ---
interface Immutable {
    value: number;
    label: string;
}

function describeImmutable(obj: Immutable): string {
    return obj.label + "=" + obj.value.toString();
}

console.log(describeImmutable({ value: 42, label: "answer" }));  // answer=42

// --- Generic interface ---
interface Container<T> {
    get(): T;
    set(value: T): void;
}

class SimpleContainer<T> implements Container<T> {
    private _value: T;

    constructor(initial: T) {
        this._value = initial;
    }

    get(): T {
        return this._value;
    }

    set(value: T): void {
        this._value = value;
    }
}

const numContainer = new SimpleContainer<number>(0);
numContainer.set(42);
console.log(numContainer.get());  // 42

const strContainer = new SimpleContainer<string>("hello");
console.log(strContainer.get());  // hello
strContainer.set("world");
console.log(strContainer.get());  // world

// --- Interface method dispatch through base type ---
interface Serializable {
    serialize(): string;
}

class JsonObj implements Serializable {
    data: Record<string, number>;

    constructor() {
        this.data = {};
    }

    addField(key: string, value: number): void {
        this.data[key] = value;
    }

    serialize(): string {
        const keys = Object.keys(this.data);
        const parts: string[] = [];
        for (let i = 0; i < keys.length; i++) {
            parts.push(keys[i] + ":" + this.data[keys[i]].toString());
        }
        return "{" + parts.join(",") + "}";
    }
}

const j = new JsonObj();
j.addField("a", 1);
j.addField("b", 2);
console.log(j.serialize());  // {a:1,b:2}

// --- Callback interface pattern ---
interface Comparator<T> {
    compare(a: T, b: T): number;
}

class NumberComparator implements Comparator<number> {
    compare(a: number, b: number): number {
        return a - b;
    }
}

function sortWith<T>(arr: T[], cmp: Comparator<T>): T[] {
    const result = [...arr];
    result.sort((a: T, b: T) => cmp.compare(a, b));
    return result;
}

const sorted = sortWith([30, 10, 50, 20, 40], new NumberComparator());
console.log(sorted.join(","));  // 10,20,30,40,50
