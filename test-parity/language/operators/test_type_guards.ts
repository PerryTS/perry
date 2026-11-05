// Test type guards (typeof-based type narrowing)

// Test 1: Basic typeof on known types
let str: string = "hello";
let num: number = 42;

console.log(typeof str);  // Should print: string
console.log(typeof num);  // Should print: number
console.log(typeof true); // Should print: boolean
// Note: typeof undefined requires global support, skipped for now
// console.log(typeof null); // typeof null === "object" requires global null support

// Test 2: typeof on union type with runtime check
function printValue(x: string | number): void {
    if (typeof x === "string") {
        console.log("It's a string");
        console.log(x);  // x is narrowed to string
    } else {
        console.log("It's a number");
        console.log(x);  // x is narrowed to number
    }
}

printValue("test");
printValue(100);

// Test 3: typeof comparison returns boolean
let x: string | number = "hello";
console.log(typeof x);  // Expected: string
let isString: boolean = typeof x === "string";
console.log(isString);  // Expected: 1 (true)

// Assign a number to the union variable
x = 42;
console.log(typeof x);  // Expected: number
isString = typeof x === "string";
console.log(isString);  // Expected: 0 (false)

// Test 4: Multiple typeof checks
function describe(val: string | number): void {
    let typeStr: string = typeof val;
    console.log(typeStr);
}

describe("foo");  // Should print: string
describe(3.14);   // Should print: number

// Test completed marker
console.log(999);
