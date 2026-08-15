A compiled production Next.js App Route no longer dies at startup with `Cannot find module './chunks/2.js'`.

Next's production webpack runtime loads lazy chunks by a *computed* relative specifier — `.next/server/webpack-runtime.js` calls `require("./chunks/" + g.u(a))`. Perry's CJS `require` shim passed that string straight to the path→module registry, which is keyed by each module's **absolute** source path, so the lookup could never hit. Statically-known relative specifiers are resolved at compile time and never reach that branch; only computed ones do, which is why this only showed up on the real production route.

The result was that every lazy chunk was unreachable at runtime even though it had been compiled into the image — all 104 modules of the #8034 fixture produced object files, `chunks/2.js` among them, and the host still exited during startup.

Computed relative specifiers are now joined against the requiring module's own directory before the registry lookup. The `./` prefix is stripped textually rather than left to `std::fs::canonicalize`, which only normalizes paths that exist on disk while registration falls back to the raw string when they do not — relying on it would work from a source tree and silently fail in the deployed case the dylib packaging exists for.

Refs #8040, #5438.
