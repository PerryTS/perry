**Compiler: preserve installed package-instance identity during native dependency compilation (#8516).**

Perry previously keyed `compilePackages` discovery by package name. Once one copy of a package had been found, every importer was redirected to that directory—even when normal Node/Bun resolution selected a different nested installation. A dependency tree containing `dup-pkg@1` at the root and `dup-pkg@2` beneath another package therefore compiled one copy and silently bound both importers to it.

Bare package resolution now walks outward from each importer, and the compiler tracks every canonical package root in a deterministic set. Distinct installed roots receive their existing path-derived module, linker-symbol, and object-cache identities; multiple links to the same physical root still deduplicate naturally. Native-package routing remains an explicit package policy and native-addon checks now run for every resolved package instance.

The regression suite links and runs a four-module two-version fixture and verifies `top-v1 nested-v2`, plus resolver/cache tests that cover discovery-order independence.
