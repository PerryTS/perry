// `class MissingCredentialError extends Error {}` in a module that binds no
// `Error` of its own extends the GLOBAL Error. When another module imported
// and constructed it, Perry looked the parent name up program-wide and bound
// it to an unrelated exported `class Error` (opencode's Ripgrep.Error), so
// `super(message)` ran that class's constructor: opencode's `models` command
// failed with `Expected object, got "Missing auth credential: apiKey"`.
// The fixtures are under fixtures/bare_error_parent/.
import * as Ripgrep from "./fixtures/bare_error_parent/ripgrep.mjs";
import { optional } from "./fixtures/bare_error_parent/route.mjs";
import { MissingCredentialError } from "./fixtures/bare_error_parent/auth.mjs";

try {
  const e: any = optional("apiKey");
  console.log("missing", e._tag, e.source, e.message, Object.keys(e).join(","));
  console.log(
    "missing chain",
    e instanceof MissingCredentialError,
    e instanceof Error,
    e instanceof Ripgrep.Error,
    Object.getPrototypeOf(MissingCredentialError) === Error,
  );
} catch (err: any) {
  console.log("missing THROWS", err.message);
}

const r: any = new Ripgrep.Error({ message: "rg" });
console.log("ripgrep", r._tag, r.message, r instanceof Error);
