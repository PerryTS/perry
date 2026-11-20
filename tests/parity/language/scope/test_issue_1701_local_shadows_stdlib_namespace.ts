// Issue #1701 — the #503 dynamic-stdlib-dispatch guard must NOT fire on a
// LOCAL variable that merely shares a name with a stdlib namespace.
//
// hono's trie-router has a `path` parameter (a URL-path string) and does
// `path[0] === "/"`. The #503 guard (which refuses `node:path`-namespace
// dynamic dispatch like `path[runtimeVar]()`) matched the local `path` by
// NAME, without checking it was actually the imported namespace, and refused
// to compile the whole `hono` package. Fix: a local binding shadowing a
// namespace name is the user's own variable, never the namespace — skip it.
//
// This exercises locals named after several stdlib namespaces, with both
// computed index reads and a computed method call (the exact shapes the guard
// targets, but on locals). All must compile and match Node.

function firstSlash(path: string): number {
    const i = 0;
    return path[i] === "/" ? 1 : 0;
}

// Local `fs` shadowing the node:fs namespace name, computed method call.
function pick(fs: Record<string, () => string>, key: string): string {
    return fs[key]();
}

// Locals named `crypto`, `os`, `url` with computed index.
function joinFirst(crypto: string[], os: number, url: string): string {
    const k = os;
    return crypto[k] + ":" + url[0];
}

console.log("firstSlash /abc:", firstSlash("/abc"));
console.log("firstSlash abc:", firstSlash("abc"));
console.log("pick:", pick({ go: () => "went" }, "go"));
console.log("joinFirst:", joinFirst(["a", "b"], 1, "xyz"));
