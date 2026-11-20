// Edge-case tests for destructuring: arrays, objects, nested, defaults,
// rest patterns, function parameters, swap pattern

// --- Array destructuring ---
const [a, b, c] = [1, 2, 3];
console.log(a);  // 1
console.log(b);  // 2
console.log(c);  // 3

// --- Skip elements ---
const [first, , third] = [10, 20, 30];
console.log(first);  // 10
console.log(third);  // 30

// --- Rest in array destructuring ---
const [head, ...tail] = [1, 2, 3, 4, 5];
console.log(head);           // 1
console.log(tail.join(","));  // 2,3,4,5

// --- Array destructuring with defaults ---
const [x = 10, y = 20, z = 30] = [1, 2] as (number | undefined)[];
console.log(x);  // 1
console.log(y);  // 2
console.log(z);  // 30

// --- Swap via destructuring ---
let sw1 = 1;
let sw2 = 2;
[sw1, sw2] = [sw2, sw1];
console.log(sw1);  // 2
console.log(sw2);  // 1

// --- Object destructuring ---
const { name, age } = { name: "Alice", age: 30 };
console.log(name);  // Alice
console.log(age);   // 30

// --- Object destructuring with rename ---
const { name: personName, age: personAge } = { name: "Bob", age: 25 };
console.log(personName);  // Bob
console.log(personAge);   // 25

// --- Object destructuring with defaults ---
const { host = "localhost", port = 8080 } = { host: "server" } as { host?: string; port?: number };
console.log(host);  // server
console.log(port);  // 8080

// --- Nested object destructuring ---
const {
    outer: { inner }
} = { outer: { inner: 42 } };
console.log(inner);  // 42

// --- Nested array destructuring ---
const [[a1, a2], [b1, b2]] = [[1, 2], [3, 4]];
console.log(a1);  // 1
console.log(a2);  // 2
console.log(b1);  // 3
console.log(b2);  // 4

// --- Mixed nested destructuring ---
const {
    coords: [cx, cy],
    label
} = { coords: [10, 20], label: "point" };
console.log(cx);     // 10
console.log(cy);     // 20
console.log(label);  // point

// --- Destructuring in function parameters ---
function greet({ name, greeting = "Hello" }: { name: string; greeting?: string }): string {
    return greeting + ", " + name + "!";
}

console.log(greet({ name: "World" }));                   // Hello, World!
console.log(greet({ name: "World", greeting: "Hi" }));   // Hi, World!

// --- Array destructuring in function parameters ---
function sum3([a, b, c]: number[]): number {
    return a + b + c;
}

console.log(sum3([10, 20, 30]));  // 60

// --- Destructuring in for...of ---
const pairs: [string, number][] = [["a", 1], ["b", 2], ["c", 3]];
for (const [key, val] of pairs) {
    console.log(key + "=" + val.toString());
}
// a=1
// b=2
// c=3

// --- Rest in object destructuring ---
const { id, ...rest } = { id: 1, name: "test", value: 42 };
console.log(id);  // 1
console.log(rest.name);   // test
console.log(rest.value);  // 42

// --- Destructuring return value ---
function getMinMax(arr: number[]): { min: number; max: number } {
    let min = arr[0];
    let max = arr[0];
    for (let i = 1; i < arr.length; i++) {
        if (arr[i] < min) min = arr[i];
        if (arr[i] > max) max = arr[i];
    }
    return { min, max };
}

const { min, max } = getMinMax([3, 1, 4, 1, 5, 9, 2, 6]);
console.log(min);  // 1
console.log(max);  // 9

// --- Destructuring with computed property names ---
const prop = "name";
const { [prop]: extractedName } = { name: "dynamic" };
console.log(extractedName);  // dynamic

// --- Multiple destructurings from same source ---
const source = { a: 1, b: 2, c: 3, d: 4 };
const { a: sa, b: sb } = source;
const { c: sc, d: sd } = source;
console.log(sa + sb + sc + sd);  // 10

// --- Destructuring array of objects ---
const people = [
    { name: "Alice", score: 95 },
    { name: "Bob", score: 87 },
    { name: "Carol", score: 92 }
];

for (let i = 0; i < people.length; i++) {
    const { name: n, score: s } = people[i];
    console.log(n + ":" + s.toString());
}
// Alice:95
// Bob:87
// Carol:92

// --- Tuple destructuring ---
function divide(a: number, b: number): [number, number] {
    return [Math.floor(a / b), a % b];
}

const [quotient, remainder] = divide(17, 5);
console.log(quotient);   // 3
console.log(remainder);  // 2

// --- Deeply nested destructuring ---
const deep = {
    level1: {
        level2: {
            items: [10, 20, 30]
        }
    }
};

const { level1: { level2: { items: [i1, i2, i3] } } } = deep;
console.log(i1);  // 10
console.log(i2);  // 20
console.log(i3);  // 30
