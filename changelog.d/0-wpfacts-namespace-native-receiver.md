Whole-program facts: a bare call of a global function (`fetch(url)`) no
longer hands the global namespace to unknown code. The call is made through
the member's native value on the unresolved-global sentinel; the native now
runs on the receiver token it is called for, so the namespace's own object
stays known (as `globalThis.fetch(url)` and `const f = fetch; f(url)` already
did), and `Promise` and the other globals read off it keep their values. A
program object the native is called on, and every argument, still reach
unknown code. In three.js this was the second link of the chain that hands
every math class to unknown code (FileLoader's `fetch( req )`).
