A `createRequire` call (or a `require` literal) of a package Perry implements
natively, such as `node-pty`, now gets the native module, the same one a static
import gets. It used to pull the package's npm files (and the Node-API addon
they load) into the program when the package was installed in `node_modules`.
A `perry.nativeAddons` entry for such a package has no effect and prints a
compile note; list the package in `perry.compilePackages` to host its addon.
The static require transform also recognises `import * as NodeModule from
"node:module"` with `NodeModule.createRequire(...)` and an immediately called
`createRequire(import.meta.url)("pkg")`.
