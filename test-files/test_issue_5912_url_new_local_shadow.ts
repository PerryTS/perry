// Issue #5912 — `new URL(...)` (and URLSearchParams/URLPattern/TextEncoder/
// TextDecoder) were dispatched by bare identifier name with no check for
// whether the name is actually the global constructor or shadowed by a
// local function/class. Real packages ship their own tolerant `URL`
// polyfill (e.g. @mixmark-io/domino's lib/URL.js calls `new URL()` with
// zero args against ITS OWN constructor) and hit perry's native URL
// constructor instead, which requires at least one argument.
//
// This exercises the exact shadowing shape: a local function named `URL`
// that tolerates a missing argument, matching Node's output.

function URL(url?: string) {
    return { url: url ?? "default", kind: "local" };
}

console.log(JSON.stringify(new URL()));
console.log(JSON.stringify(new URL("explicit")));

function withTextEncoder() {
    function TextEncoder(label?: string) {
        return { label: label ?? "utf-8", kind: "local" };
    }
    return new TextEncoder();
}

console.log(JSON.stringify(withTextEncoder()));
