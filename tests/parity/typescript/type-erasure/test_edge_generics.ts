// Edge-case tests for generics: type parameters, constraints, inference,
// generic classes, generic functions, generic interfaces

// --- Simple generic function ---
function identity<T>(x: T): T {
    return x;
}

console.log(identity<number>(42));     // 42
console.log(identity<string>("hi"));   // hi
console.log(identity(true));           // true (inferred)

// --- Generic function with constraint ---
function getLength<T extends { length: number }>(x: T): number {
    return x.length;
}

console.log(getLength("hello"));     // 5
console.log(getLength([1, 2, 3]));   // 3

// --- Generic pair / tuple ---
function makePair<A, B>(a: A, b: B): [A, B] {
    return [a, b];
}

const pair = makePair("hello", 42);
console.log(pair[0]);  // hello
console.log(pair[1]);  // 42

// --- Generic class ---
class Box<T> {
    value: T;

    constructor(value: T) {
        this.value = value;
    }

    get(): T {
        return this.value;
    }

    map<U>(fn: (val: T) => U): Box<U> {
        return new Box<U>(fn(this.value));
    }
}

const numBox = new Box<number>(42);
console.log(numBox.get());  // 42

const strBox = numBox.map((n: number) => n.toString());
console.log(strBox.get());  // 42

const boolBox = new Box(true);
console.log(boolBox.get());  // true

// --- Generic stack ---
class Stack<T> {
    items: T[];

    constructor() {
        this.items = [];
    }

    push(item: T): void {
        this.items.push(item);
    }

    pop(): T | undefined {
        return this.items.pop();
    }

    peek(): T | undefined {
        return this.items.length > 0 ? this.items[this.items.length - 1] : undefined;
    }

    size(): number {
        return this.items.length;
    }

    isEmpty(): boolean {
        return this.items.length === 0;
    }
}

const numStack = new Stack<number>();
numStack.push(1);
numStack.push(2);
numStack.push(3);
console.log(numStack.peek());   // 3
console.log(numStack.size());   // 3
console.log(numStack.pop());    // 3
console.log(numStack.size());   // 2

const strStack = new Stack<string>();
strStack.push("a");
strStack.push("b");
console.log(strStack.isEmpty());  // false
console.log(strStack.pop());     // b
console.log(strStack.pop());     // a
console.log(strStack.isEmpty()); // true

// --- Generic function with multiple type params ---
function zip<A, B>(as: A[], bs: B[]): [A, B][] {
    const result: [A, B][] = [];
    const len = Math.min(as.length, bs.length);
    for (let i = 0; i < len; i++) {
        result.push([as[i], bs[i]]);
    }
    return result;
}

const zipped = zip([1, 2, 3], ["a", "b", "c"]);
for (let i = 0; i < zipped.length; i++) {
    console.log(zipped[i][0] + ":" + zipped[i][1]);
}
// 1:a
// 2:b
// 3:c

// --- Generic with default handling ---
function firstOrDefault<T>(arr: T[], defaultVal: T): T {
    return arr.length > 0 ? arr[0] : defaultVal;
}

console.log(firstOrDefault([10, 20], 0));        // 10
console.log(firstOrDefault([] as number[], 99));  // 99
console.log(firstOrDefault(["a", "b"], "none"));  // a

// --- Generic map function ---
function mapArray<T, U>(arr: T[], fn: (item: T) => U): U[] {
    const result: U[] = [];
    for (let i = 0; i < arr.length; i++) {
        result.push(fn(arr[i]));
    }
    return result;
}

const squared = mapArray([1, 2, 3, 4], (x: number) => x * x);
console.log(squared.join(","));  // 1,4,9,16

const lengths = mapArray(["hello", "hi", "hey"], (s: string) => s.length);
console.log(lengths.join(","));  // 5,2,3

// --- Generic filter function ---
function filterArray<T>(arr: T[], pred: (item: T) => boolean): T[] {
    const result: T[] = [];
    for (let i = 0; i < arr.length; i++) {
        if (pred(arr[i])) {
            result.push(arr[i]);
        }
    }
    return result;
}

const bigNums = filterArray([1, 5, 10, 15, 20], (n: number) => n > 8);
console.log(bigNums.join(","));  // 10,15,20

// --- Generic reduce ---
function reduceArray<T, U>(arr: T[], fn: (acc: U, item: T) => U, init: U): U {
    let acc = init;
    for (let i = 0; i < arr.length; i++) {
        acc = fn(acc, arr[i]);
    }
    return acc;
}

const total = reduceArray([1, 2, 3, 4], (acc: number, n: number) => acc + n, 0);
console.log(total);  // 10

const concat = reduceArray(["a", "b", "c"], (acc: string, s: string) => acc + s, "");
console.log(concat);  // abc

// --- Generic class with inheritance ---
class Collection<T> {
    protected items: T[];

    constructor() {
        this.items = [];
    }

    add(item: T): void {
        this.items.push(item);
    }

    getAll(): T[] {
        return this.items;
    }

    count(): number {
        return this.items.length;
    }
}

class SortedCollection extends Collection<number> {
    add(item: number): void {
        this.items.push(item);
        this.items.sort((a: number, b: number) => a - b);
    }
}

const sorted = new SortedCollection();
sorted.add(30);
sorted.add(10);
sorted.add(20);
console.log(sorted.getAll().join(","));  // 10,20,30

// --- Generic with Record type ---
function groupBy<T>(arr: T[], keyFn: (item: T) => string): Record<string, T[]> {
    const result: Record<string, T[]> = {};
    for (let i = 0; i < arr.length; i++) {
        const key = keyFn(arr[i]);
        if (!result[key]) {
            result[key] = [];
        }
        result[key].push(arr[i]);
    }
    return result;
}

const grouped = groupBy([1, 2, 3, 4, 5, 6], (n: number) => n % 2 === 0 ? "even" : "odd");
console.log(grouped["even"].join(","));  // 2,4,6
console.log(grouped["odd"].join(","));   // 1,3,5

// --- Chained generic operations ---
const chainResult = mapArray(
    filterArray([1, 2, 3, 4, 5, 6, 7, 8], (n: number) => n % 2 === 0),
    (n: number) => n * n
);
console.log(chainResult.join(","));  // 4,16,36,64
