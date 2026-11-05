// Test error extensions: cause, AggregateError, custom subclasses, stack, typed errors

// Error.cause property (ES2022)
const original = new Error("original failure");
const wrapped = new Error("wrapper", { cause: original });
console.log(wrapped.message); // wrapper
console.log((wrapped as any).cause.message); // original failure

// AggregateError
const err1 = new Error("first");
const err2 = new Error("second");
const agg = new AggregateError([err1, err2], "multiple failures");
console.log(agg.message); // multiple failures
console.log(agg.errors.length); // 2
console.log(agg.errors[0].message); // first
console.log(agg.errors[1].message); // second

// Custom error subclass with extra properties
class HttpError extends Error {
  statusCode: number;
  constructor(message: string, statusCode: number) {
    super(message);
    this.name = "HttpError";
    this.statusCode = statusCode;
  }
}

const httpErr = new HttpError("Not Found", 404);
console.log(httpErr.message); // Not Found
console.log(httpErr.statusCode); // 404
console.log(httpErr.name); // HttpError
console.log(httpErr instanceof HttpError); // true
console.log(httpErr instanceof Error); // true

// Error .stack property exists as string
const stackErr = new Error("stack test");
console.log(typeof stackErr.stack); // string
console.log(stackErr.stack!.includes("stack test")); // true

// TypeError
const typeErr = new TypeError("not a function");
console.log(typeErr.message); // not a function
console.log(typeErr instanceof TypeError); // true
console.log(typeErr instanceof Error); // true

// RangeError
const rangeErr = new RangeError("out of bounds");
console.log(rangeErr.message); // out of bounds
console.log(rangeErr instanceof RangeError); // true
console.log(rangeErr instanceof Error); // true

// ReferenceError
const refErr = new ReferenceError("x is not defined");
console.log(refErr.message); // x is not defined
console.log(refErr instanceof ReferenceError); // true

// SyntaxError
const synErr = new SyntaxError("unexpected token");
console.log(synErr.message); // unexpected token
console.log(synErr instanceof SyntaxError); // true

// instanceof checks across types
console.log(typeErr instanceof RangeError); // false
console.log(rangeErr instanceof TypeError); // false

// Expected output:
// wrapper
// original failure
// multiple failures
// 2
// first
// second
// Not Found
// 404
// HttpError
// true
// true
// string
// true
// not a function
// true
// true
// out of bounds
// true
// true
// x is not defined
// true
// unexpected token
// true
// false
// false
