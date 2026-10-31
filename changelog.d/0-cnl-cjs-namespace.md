A CommonJS module's namespace now holds exactly the keys the module writes to
`exports`/`module.exports` (plus `default` and `module.exports`), as in Node.
The CJS-to-ESM wrap hoists top-level classes to module scope for class
identity, and it used to export every hoisted class: a private helper class
(`@aws-sdk/core` protocols' `SerdeContextConfig`, `UnionSerde`) showed up in
`import * as ns`, and `module.exports = Class` also exported `Class` by name. A
hoisted class is now a direct export only under a key the module writes, when
every write of that key stores the class binding itself (`exports.X = X`,
`{ X }`, esbuild's `X: () => X`) and the binding is never reassigned. Any other
key value, such as `exports.X = wrap(X)`, is read from `module.exports` instead
of being replaced by the class.
