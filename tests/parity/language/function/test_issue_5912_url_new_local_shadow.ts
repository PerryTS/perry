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

// CodeRabbit follow-up on the #5913 PR — an explicit `globalThis.` qualifier
// is an escape hatch to the REAL global and must keep working even while the
// bare `URL` identifier is shadowed above.
console.log(new (globalThis as any).URL("https://example.com/path").hostname);

// CodeRabbit follow-up — a local alias of the shadowed name must not resolve
// back to the native constructor either (`resolve_class_alias` is name-keyed,
// not scope-aware, so this needs its own explicit check).
const MyURL = URL;
console.log(JSON.stringify(new MyURL("via-alias")));
