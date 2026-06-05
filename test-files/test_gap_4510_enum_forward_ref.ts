// #4510: an enum referenced before its textual declaration must resolve to the
// real member value, not silently lower to 0. Enum bindings are module-scoped
// in TypeScript, so a function (or earlier statement) declared above the enum
// may legally reference it.

// String enum used before declaration (the issue's minimal repro).
function tag(): First {
    return First.B;
}
enum First {
    A = "A",
    B = "B",
    C = "C",
}
console.log("fwd:", tag()); // B

// Numeric enum referenced before declaration.
function num(): number {
    return Second.Y;
}
enum Second {
    X = 1,
    Y = 2,
    Z = 3,
}
console.log("num:", num()); // 2

// Auto-incremented enum referenced before declaration.
function auto(): number {
    return Third.C;
}
enum Third {
    A,
    B,
    C,
}
console.log("auto:", auto()); // 2

// A top-level `const` initialized from an enum member declared later.
const before = Fourth.GREEN;
enum Fourth {
    RED = "red",
    GREEN = "green",
}
console.log("before:", before); // green

// Switch dispatch against a forward-declared enum (the zod pattern).
function kind(k: Kind): string {
    switch (k) {
        case Kind.A:
            return "isA";
        case Kind.B:
            return "isB";
        default:
            return "other";
    }
}
enum Kind {
    A = "a",
    B = "b",
}
console.log("dispatch:", kind(Kind.B)); // isB

// Backward reference (already worked) keeps working.
enum Fifth {
    ON = "on",
    OFF = "off",
}
console.log("back:", Fifth.OFF); // off
