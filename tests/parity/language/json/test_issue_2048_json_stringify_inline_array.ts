// Issue #2048: a program that walks a prototype chain into a function-local
// array and then calls `JSON.stringify(walk(...))` inline twice (once on a
// class instance, once on a plain Array) used to SIGSEGV (or hang in a
// `.join()` variant) under `PERRY_NO_AUTO_OPTIMIZE=1`. The crash was the
// severe form of #2047: deforest's local `walk_expr_children` treated
// `JsonStringifyFull` (and other less-common `Expr` variants) as a leaf,
// so the producer call `walk(...)` hidden inside an inline
// `JSON.stringify(...)` slipped past the unsafe-call-site scan. The
// candidate got rewritten to take a trailing accumulator argument and
// `return out` became `return undefined`, but the inline expression-
// position caller still passed a single argument — heap corruption
// followed. The #2047 fix (delegate to perry-hir's exhaustive walker)
// also resolves this case; this gap test pins it in place.

const kind = Symbol.for("k");
class Base { static [kind] = "Base"; }
class Leaf extends Base { static [kind] = "Leaf"; }

function walk(value: any): string[] {
    const seen: string[] = [];
    let cls = Object.getPrototypeOf(value).constructor;
    while (cls) {
        if (kind in cls) seen.push(cls[kind]);
        cls = Object.getPrototypeOf(cls);
    }
    return seen;
}

// Two inline JSON.stringify calls — the second one used to crash before
// the deforest walker fix.
console.log("leaf:", JSON.stringify(walk(new Leaf())));
console.log("array:", JSON.stringify(walk([1, 2, 3])));

// The `.join()` variant the issue called out as hanging instead of
// crashing — keep it here so we catch a regression in either direction.
const a = walk(new Leaf());
console.log("leaf join:", a.join(","));
const b = walk([1, 2, 3]);
console.log("array join:", b.join(","));
