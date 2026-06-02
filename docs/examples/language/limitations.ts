// demonstrates: TypeScript subset shown in docs/src/language/limitations.md
// docs: docs/src/language/limitations.md
// platforms: macos, linux, windows

// Each ANCHOR block below is the exact code that the limitations docs
// page renders inline (via {{#include ... :NAME}}). Snippets are wrapped
// in their own functions so top-level identifiers don't collide across
// anchors. Snippets that demonstrate intentional rejection (eval, runtime
// metadata, dynamic require, prototype mutation) cannot be compile-verified
// by definition — those stay as `,no-test` in the markdown body.

// ANCHOR: erased-types
function someFunction(): number {
    return 42
}

function erasedTypes(): void {
    // These annotations are erased — no runtime effect
    const x: number = someFunction(); // No runtime check that result is actually a number
    console.log(`erased-types: x=${x}`)
}
// ANCHOR_END: erased-types

// ANCHOR: error-subclass
class CustomError extends Error {
    code: number;
    constructor(msg: string, code: number) {
        super(msg);
        this.code = code;
    }
}
// ANCHOR_END: error-subclass

function errorSubclassDemo(): void {
    // Works
    try {
        throw new Error("message");
    } catch (e: any) {
        console.log(`caught: ${e.message}`)
    }

    try {
        throw new CustomError("custom", 42)
    } catch (e: any) {
        console.log(`caught custom: ${e.message}`)
    }
}

// ANCHOR: computed-keys-supported
function computedKeysSupported(): void {
    const obj: { [k: string]: any } = {}
    const key = "name";
    obj[key] = "value";

    console.log(`computed-keys-supported: ${obj[key]}`)
}
// ANCHOR_END: computed-keys-supported

// ANCHOR: type-narrowing
function processValue(value: string | number) {
    // Instead of relying on type narrowing from generics
    if (typeof value === "string") {
        // String path
        console.log(`string path: ${value}`)
    } else if (typeof value === "number") {
        // Number path
        console.log(`number path: ${value}`)
    }
}
// ANCHOR_END: type-narrowing

// driver
erasedTypes()
errorSubclassDemo()
computedKeysSupported()
processValue("hi")
processValue(42)
