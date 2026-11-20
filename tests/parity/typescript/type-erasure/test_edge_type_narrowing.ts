// Edge-case tests for type narrowing, type guards, union types, and type inference
// Targets bugs like: union-typed obj[key] wrong dispatch, any-typed array element
// property access, type guard with 'in' operator

// --- typeof narrowing ---
function describeValue(x: string | number | boolean): string {
    if (typeof x === "string") {
        return "string:" + x.toUpperCase();
    } else if (typeof x === "number") {
        return "number:" + (x * 2).toString();
    } else {
        return "boolean:" + (x ? "yes" : "no");
    }
}

console.log(describeValue("hello"));  // string:HELLO
console.log(describeValue(21));       // number:42
console.log(describeValue(true));     // boolean:yes
console.log(describeValue(false));    // boolean:no

// --- Narrowing with equality checks ---
function process(x: string | null | undefined): string {
    if (x === null) return "null";
    if (x === undefined) return "undefined";
    return x.toUpperCase();
}

console.log(process("hello"));    // HELLO
console.log(process(null));       // null
console.log(process(undefined));  // undefined

// --- Truthiness narrowing ---
function processOptional(x: string | null): string {
    if (x) {
        return x.toUpperCase();
    }
    return "empty";
}

console.log(processOptional("hello"));  // HELLO
console.log(processOptional(null));     // empty
console.log(processOptional(""));       // empty

// --- 'in' operator type guard ---
interface Circle {
    kind: string;
    radius: number;
}

interface Rect {
    kind: string;
    width: number;
    height: number;
}

function getArea(shape: Circle | Rect): number {
    if ("radius" in shape) {
        return Math.PI * shape.radius * shape.radius;
    } else {
        return shape.width * shape.height;
    }
}

const circle: Circle = { kind: "circle", radius: 5 };
const rect: Rect = { kind: "rect", width: 3, height: 4 };
console.log(Math.round(getArea(circle)));  // 79
console.log(getArea(rect));                // 12

// --- instanceof narrowing ---
class Dog2 {
    bark(): string { return "Woof!"; }
}

class Cat {
    meow(): string { return "Meow!"; }
}

function makeSound(animal: Dog2 | Cat): string {
    if (animal instanceof Dog2) {
        return animal.bark();
    } else {
        return animal.meow();
    }
}

console.log(makeSound(new Dog2()));  // Woof!
console.log(makeSound(new Cat()));   // Meow!

// --- Discriminated unions ---
interface Success {
    status: string;
    data: string;
}

interface Failure {
    status: string;
    error: string;
}

function handleResult(result: Success | Failure): string {
    if (result.status === "ok") {
        return "Data: " + (result as Success).data;
    } else {
        return "Error: " + (result as Failure).error;
    }
}

console.log(handleResult({ status: "ok", data: "hello" }));       // Data: hello
console.log(handleResult({ status: "err", error: "not found" })); // Error: not found

// --- Union of primitives with array ---
function sumOrConcat(items: (number | string)[]): string {
    let numSum = 0;
    let strParts: string[] = [];

    for (let i = 0; i < items.length; i++) {
        const item = items[i];
        if (typeof item === "number") {
            numSum = numSum + item;
        } else {
            strParts.push(item);
        }
    }

    return numSum.toString() + ":" + strParts.join(",");
}

console.log(sumOrConcat([1, "a", 2, "b", 3]));  // 6:a,b

// --- Optional properties ---
interface User {
    name: string;
    email?: string;
    age?: number;
}

function describeUser(user: User): string {
    let desc = user.name;
    if (user.email) {
        desc = desc + " <" + user.email + ">";
    }
    if (user.age !== undefined) {
        desc = desc + " age:" + user.age.toString();
    }
    return desc;
}

console.log(describeUser({ name: "Alice" }));                            // Alice
console.log(describeUser({ name: "Bob", email: "bob@test.com" }));       // Bob <bob@test.com>
console.log(describeUser({ name: "Carol", age: 30 }));                   // Carol age:30
console.log(describeUser({ name: "Dave", email: "d@t.com", age: 25 })); // Dave <d@t.com> age:25

// --- Type assertion ---
function getStringLength(x: unknown): number {
    if (typeof x === "string") {
        return (x as string).length;
    }
    return -1;
}

console.log(getStringLength("hello"));  // 5
console.log(getStringLength(42));       // -1

// --- Narrowing in switch ---
function switchNarrowing(x: string | number | boolean): string {
    switch (typeof x) {
        case "string":
            return "s:" + x;
        case "number":
            return "n:" + x.toString();
        case "boolean":
            return "b:" + (x ? "T" : "F");
        default:
            return "unknown";
    }
}

console.log(switchNarrowing("hi"));    // s:hi
console.log(switchNarrowing(42));      // n:42
console.log(switchNarrowing(false));   // b:F

// --- Nullable return type ---
function findItem(arr: number[], target: number): number | null {
    for (let i = 0; i < arr.length; i++) {
        if (arr[i] === target) return arr[i];
    }
    return null;
}

const found1 = findItem([1, 2, 3], 2);
const found2 = findItem([1, 2, 3], 5);
console.log(found1 !== null ? found1.toString() : "not found");  // 2
console.log(found2 !== null ? found2.toString() : "not found");  // not found

// --- Array with union element type ---
const mixedArr: (number | string)[] = [1, "two", 3, "four"];
for (let i = 0; i < mixedArr.length; i++) {
    const item = mixedArr[i];
    if (typeof item === "number") {
        console.log("num:" + item.toString());
    } else {
        console.log("str:" + item);
    }
}
// num:1
// str:two
// num:3
// str:four

// --- Nested narrowing ---
interface Nested {
    outer?: {
        inner?: {
            value: number;
        };
    };
}

function getNestedValue(obj: Nested): number {
    if (obj.outer && obj.outer.inner) {
        return obj.outer.inner.value;
    }
    return -1;
}

console.log(getNestedValue({ outer: { inner: { value: 42 } } }));  // 42
console.log(getNestedValue({ outer: {} }));                          // -1
console.log(getNestedValue({}));                                     // -1

// --- Non-null assertion pattern ---
function getFirst(arr: number[]): number {
    if (arr.length === 0) return -1;
    return arr[0];
}

console.log(getFirst([10, 20]));  // 10
console.log(getFirst([]));        // -1

// --- Union with methods ---
function processInput(input: string | number[]): string {
    if (typeof input === "string") {
        return input.toUpperCase();
    } else {
        return input.join(",");
    }
}

console.log(processInput("hello"));     // HELLO
console.log(processInput([1, 2, 3]));   // 1,2,3
