`export * as ns from "./m.ts"` now appears on the re-exporting module's
namespace object.

The binding existed, but only under one of the two ways of reaching it:

```ts
// mid.ts
export * as deep from "./leaf.ts";

// main.ts
import { deep } from "./mid.ts";     // worked
import * as B from "./mid.ts";
B.deep;                              // undefined
Object.keys(B);                      // "deep" missing entirely
```

The practical cost was zod. Its `external.ts` re-exports four sub-namespaces
exactly this way, so `z.coerce`, `z.iso`, `z.core` and `z.locales` were all
undefined and any code touching them died with "Cannot read properties of
undefined".

Two things were missing. A namespace alias has no declaring binding to point
at — it names a whole module, not something inside one — so it never entered
the export surface the compiler builds for a namespace import, which is why
`Object.keys` did not list it. And every way of resolving a namespace member
ended in a symbol to call, which an alias does not have, so the read fell
through to a generic property lookup on an object that had no such key.

The fix reuses the object the compiler already knows how to build. Perry
already emits a populated namespace object for any module reachable by a
dynamic `import()`, and that populator already handles nested namespaces
recursively. So a namespace re-export target now gets one too, and `B.deep`
loads it. Nesting several levels deep works for the same reason: it is the
same populator all the way down, not a second implementation that has to
reproduce the first one's behaviour.

Members reached through the object behave normally — `B.deep.alpha`,
`B.deep.beta()`, and `Object.keys(B.deep)` all match Node.

Closes #7189.
