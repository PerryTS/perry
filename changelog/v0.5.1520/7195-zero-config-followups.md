### Fixed

- **The auto-compile default no longer adds a line to every build's stdout
  (#7137 follow-up).** Injecting the implicit `"*"` made the wildcard
  expansion block run on every text-mode compile, so even a program with no
  `node_modules` at all printed `Compile package wildcard: expanded to 0
  installed package(s)`. A host that spelled a wildcard out (`["*"]`,
  `"auto"`, `"@scope/*"`) still always gets the report; the implicit default
  reports only when the expansion actually routed or skipped something.

- **Perry now warns when zero-config resolution hands an importer a
  different package version than Node would.** For any package in
  `perry.compilePackages`, `resolve_import` searches the project root before
  the importer's own ancestors (so a top-level ESM copy beats a nested CJS
  one), and `compile_package_dirs` then keeps one directory per package name.
  Both were narrow while the set held only hand-listed packages — opting one
  in was a deliberate act. The auto-compile default puts the whole reachable
  graph in that set, so both now apply to every bare specifier, and a tree
  carrying two majors of one package silently gets one of them. Verified: a
  project with `dup-pkg@1.0.0` at the top level and `dup-pkg@2.0.0` under
  `sub/node_modules` prints `A-top-level-v1 / B-nested-v2` under Node 26.5.1
  and `A-top-level-v1 / A-top-level-v1` under Perry. Perry now emits one
  warning per package naming both versions, both paths, and the importer that
  was redirected. Identical versions (a genuine duplicate install) and copies
  with no readable `version` stay silent.
