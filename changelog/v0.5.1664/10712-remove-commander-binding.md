**Removed the native `commander` binding** — `import { Command } from "commander"` now resolves to
the real npm package, compiled from source. Native `program.args` was `undefined`; boolean option
defaults serialized as the truthy string `"false"`; subcommand `.action()` callbacks never fired;
missing-required-argument and unknown-option validation (Node's `commander.missingArgument` /
`commander.unknownOption`) was entirely absent. `class Command extends EventEmitter` in the real
source needs no dedicated native-subclass support — Perry's existing generic EventEmitter-subclass
machinery already covers it. Fixes #10686. Requires #10439's import-provenance fix (#10699) to reach
the real package at its default import name.

Acceptance against the real npm `commander@15.0.0` (installed as the only dependency, **no**
`perry.compilePackages` entry) vs `node --experimental-strip-types` on the pinned Node 26.5.1:
**stdout byte-identical**, covering options with defaults and `--no-` negation, `.opts()`,
`program.args` / `processedArgs`, subcommand `.argument()` + `.action()` dispatch, variadic
positionals, `helpInformation()`, `--help` routed through `configureOutput({ writeOut })`, and the
`commander.unknownOption` / `commander.missingMandatoryOptionValue` / `commander.missingArgument` /
`commander.helpDisplayed` codes thrown under `exitOverride()`.

Stderr still differs: commander's error-message *text* (`error: unknown option '--nope'`, …) is
dropped. That is #10711 — a general defect where an object-property function silently drops its
call to a second function passed to it as a parameter — not a commander one, and not a regression:
the deleted native binding contained no `stderr` write at all and did no validation, so that text
was equally absent before. Every value commander computes now matches Node; only the printed
message is missing.
