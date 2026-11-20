// Helper module for multi-module edge case tests
// Tests cross-module: function exports, class exports, generic exports,
// default parameters, re-exports, and value types

// --- Exported functions ---
export function add(a: number, b: number): number {
    return a + b;
}

export function multiply(a: number, b: number): number {
    return a * b;
}

// --- Exported function with default parameter ---
export function greet(name: string, greeting: string = "Hello"): string {
    return greeting + ", " + name + "!";
}

// --- Exported function with default array parameter (regression target) ---
export function processItems(items: number[] = []): number {
    let sum = 0;
    for (let i = 0; i < items.length; i++) {
        sum = sum + items[i];
    }
    return sum;
}

// --- Exported class ---
export class Counter {
    count: number;

    constructor(initial: number = 0) {
        this.count = initial;
    }

    increment(): void {
        this.count = this.count + 1;
    }

    decrement(): void {
        this.count = this.count - 1;
    }

    get(): number {
        return this.count;
    }
}

// --- Exported class with inheritance ---
export class Shape {
    kind: string;
    constructor(kind: string) {
        this.kind = kind;
    }
    area(): number {
        return 0;
    }
    describe(): string {
        return this.kind + ":" + this.area().toString();
    }
}

export class Circle extends Shape {
    radius: number;
    constructor(r: number) {
        super("circle");
        this.radius = r;
    }
    area(): number {
        return Math.PI * this.radius * this.radius;
    }
}

export class Rectangle extends Shape {
    width: number;
    height: number;
    constructor(w: number, h: number) {
        super("rect");
        this.width = w;
        this.height = h;
    }
    area(): number {
        return this.width * this.height;
    }
}

// --- Exported constants ---
export const PI = 3.14159265358979;
export const MAX_SIZE = 100;
export const GREETING = "Hello World";

// --- Exported generic function ---
export function identity<T>(x: T): T {
    return x;
}

export function firstOrDefault<T>(arr: T[], defaultVal: T): T {
    return arr.length > 0 ? arr[0] : defaultVal;
}

// --- Exported function returning closure ---
export function makeMultiplier(factor: number): (x: number) => number {
    return (x: number) => x * factor;
}

// --- Exported function returning object ---
export function makePoint(x: number, y: number): { x: number; y: number } {
    return { x, y };
}

// --- Exported module-level array ---
export const sharedItems: number[] = [10, 20, 30, 40, 50];

// --- Exported function that uses module-level state ---
let callCount = 0;
export function getCallCount(): number {
    callCount++;
    return callCount;
}

// --- Exported function with array return ---
export function range(start: number, end: number): number[] {
    const result: number[] = [];
    for (let i = start; i < end; i++) {
        result.push(i);
    }
    return result;
}

// --- Exported type-checking functions ---
export function isPositive(n: number): boolean {
    return n > 0;
}

export function isEven(n: number): boolean {
    return n % 2 === 0;
}
