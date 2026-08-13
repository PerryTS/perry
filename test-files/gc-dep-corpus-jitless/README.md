# The dependency-scale GC-rooting corpus

`scripts/gc_root_dominance_corpus.sh` compiles ~99 hand-written `test-files/`
sources. Each is a few dozen lines, written to exercise one lowering. That
corpus reads **zero** violations in the two modes `gc-root-dominance.yml` gates
on, and it has read zero while a twenty-line program that imports a stock npm
package faulted deterministically (#7280).

That is not a paradox, it is a distribution problem. The two corpora do not
contain the same code:

| corpus | what dominates its `--stale-registers --moving-only` report |
|---|---|
| curated (`gc_root_dominance_corpus.sh`) | property-GET helper windows, `js_number_coerce`, `js_closure_callN` |
| dependency-scale (this directory) | `js_array_alloc → js_array_spread_append`, `js_box_get_bits → js_closure_callN`, `js_object_alloc → js_object_set_field_by_name` |

A hand-written test allocates a couple of objects and calls a couple of
helpers. A real library allocates in loops, spreads arrays into arrays, boxes
every mutable capture because its closures outlive their frames, and builds
objects field by field from data. Those are different *shapes*, and the rooting
hazards live in the shapes.

So this corpus is generated from a real npm dependency —
`zod`, the repo's own `package.json` devDependency, pinned by
`package-lock.json` — rather than from anything written for the occasion. It is
emitted by `scripts/gc_root_dominance_dep_corpus.sh`.

## Layout

`main.ts` is the only entry point. It pulls in three shapes at once so that one
compile produces the whole corpus:

* **the library itself** — `zod`'s own modules, imported by source path
  (`node_modules/zod/src/index.js`), which is what makes them *native* modules
  rather than a V8-fallback bundle. This is where the module count comes from
  and it is what the distribution above is about.
* **an app over the library** — `shared.ts` plus the three endpoint modules,
  which build a registry at MODULE INIT time out of cross-module calls whose
  arguments are string literals, object literals, closures and schemas. That is
  the frame shape #7154's disassembly named.
* **a parse loop** — the twenty-line library-only control from #7280, which is
  the program that failed 5/40 while the curated corpus passed 25/25.

## These files are not gap tests

They are corpus inputs: they are compiled for their **IR**, and the compile is
the assertion. `run_parity_tests.sh` globs `test-files` at `-maxdepth 1`, so a
subdirectory is out of its scope by construction — which is deliberate, because
these need `node_modules/` and would otherwise be a parity failure on any
checkout that has not run `npm ci`.
