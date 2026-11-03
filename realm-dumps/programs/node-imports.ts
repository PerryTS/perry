// Namespace imports are realm roots: R carries each module's import name
// ("node:<module>") and its members, and the restricted dumper keeps exactly
// those imports. node:events has a native receiver cell
// (%NativeNamespace:events%); node:fs and node:path/posix do not spell one
// under their specifier, so only the import name ties them to R.
import * as events from "node:events";
import * as fs from "node:fs";
import * as posix from "node:path/posix";
const name: string = process.argv[2] ?? "world";
console.log(typeof events.EventEmitter, typeof fs.existsSync, posix.join("a", name));
