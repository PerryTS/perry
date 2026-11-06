// Edge-case tests for Buffer.from(string, encoding) and Buffer.prototype.toString(encoding).
//
// The encoding argument in Node.js is a STRING ('utf8' | 'hex' | 'base64' | ...),
// NOT a number. Historically the perry codegen fed the encoding expression through
// `fcvt_to_sint_sat` on an f64, which yielded garbage bits for string inputs, so
// the runtime always fell through to the UTF-8 default — meaning
// `Buffer.from(b64, 'base64')` silently copied the base64 text verbatim instead of
// decoding it. This test exercises the happy paths for each supported encoding.

// --- Buffer.from(string, 'base64') -> decoded bytes -> UTF-8 string ---
console.log(Buffer.from("SGVsbG8gd29ybGQ=", "base64").toString());           // Hello world
console.log(Buffer.from("SGVsbG8gd29ybGQ=", "base64").toString("utf8"));     // Hello world
console.log(Buffer.from("SGVsbG8gd29ybGQ=", "base64").length);                // 11

// --- Buffer.from(string, 'hex') -> decoded bytes ---
console.log(Buffer.from("48656c6c6f", "hex").toString());                    // Hello
console.log(Buffer.from("48656c6c6f", "hex").length);                         // 5

// --- Buffer.from(string) default (UTF-8) still works ---
console.log(Buffer.from("Hello").length);                                     // 5
console.log(Buffer.from("Hello").toString());                                 // Hello

// --- Explicit 'utf8' / 'utf-8' literal works ---
console.log(Buffer.from("Hello", "utf8").toString());                        // Hello
console.log(Buffer.from("Hello", "utf-8").toString());                       // Hello

// --- Round-trip: raw string -> Buffer -> base64 string -> Buffer -> raw string ---
const original = "Hello, perry!";
const b64 = Buffer.from(original).toString("base64");
console.log(b64);                                                             // SGVsbG8sIHBlcnJ5IQ==
console.log(Buffer.from(b64, "base64").toString());                          // Hello, perry!

// --- Round-trip: raw string -> Buffer -> hex string -> Buffer -> raw string ---
const hex = Buffer.from("abc").toString("hex");
console.log(hex);                                                             // 616263
console.log(Buffer.from(hex, "hex").toString());                             // abc

// --- Non-literal encoding argument (runtime helper path) ---
// The codegen cannot fold this at compile time, so it goes through
// js_encoding_tag_from_value at runtime.
const enc: string = "base64";
console.log(Buffer.from("cGVycnk=", enc).toString());                        // perry

const encHex: string = "hex";
console.log(Buffer.from("7065727279", encHex).toString());                   // perry
