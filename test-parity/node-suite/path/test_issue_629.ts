// Issue #629: namespace imports for unresolved node:* sub-paths used to
// silently substitute a typeof-"object" empty stub (and before that, a
// TAG_TRUE boolean). Both behaviors produced confusing downstream errors
// — silent no-ops when methods were called on the empty namespace, or
// "(boolean).X is not a function" before that. Perry now hard-errors at
// compile time when `import * as X from "..."` can't be resolved, forcing
// the user to switch to named imports or wire stdlib bindings.
//
// This file exercises the path that DOES work: bare side-effect imports
// of unresolved modules still warn-and-continue (compile success); only
// the namespace form is fatal. To regression-test the error path itself,
// see the compile-error harness; running `perry test-files/test_issue_629_repro.ts`
// against `import * as fsp from "node:fs/promises"` should fail to compile.
//
// Surfaces that `node:` builtins backed by perry-stdlib (fs, path, os,
// crypto, http, etc.) still work as before — only the unbacked sub-paths
// (fs/promises, dns, stream/promises, console, test) hard-error.
import * as path from "node:path";
console.log("path.sep:", path.sep);
console.log("path.delimiter:", path.delimiter);
