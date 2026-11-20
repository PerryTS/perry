Named exports from Node builtins now survive a local facade module. Both
`export { createHash } from "node:crypto"` and the equivalent import-then-export
form publish a live getter for the builtin ESM export value, instead of linking
to a nonexistent local function or returning an `undefined` stub. This unblocks
ethers' crypto facade and other packages that wrap Node builtins (#10432,
#10802).
