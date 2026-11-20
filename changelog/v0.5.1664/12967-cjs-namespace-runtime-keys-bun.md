Under `--platform bun`, `import * as ns` of a CommonJS module now exposes every
own key of the evaluated `module.exports`, as Bun does, not only the names the
static export lexer finds. A UMD package (fuzzysort) or `module.exports = make()`
left `ns.go` undefined, which crashed opencode's command palette and model
dialog on the first filter keystroke. Node-platform builds keep Node's semantics.
