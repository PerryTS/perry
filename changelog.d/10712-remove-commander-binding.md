**Removed the native `commander` binding** — `import { Command } from "commander"` now resolves to
the real npm package, compiled from source. Native `program.args` was `undefined`; boolean option
defaults serialized as the truthy string `"false"`; subcommand `.action()` callbacks never fired;
missing-required-argument and unknown-option validation (Node's `commander.missingArgument` /
`commander.unknownOption`) was entirely absent. `class Command extends EventEmitter` in the real
source needs no dedicated native-subclass support — Perry's existing generic EventEmitter-subclass
machinery already covers it. Fixes #10686. Requires #10439's import-provenance fix (#10699) to reach
the real package at its default import name.
