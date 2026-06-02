// Issue #35 / #321: models effect's same-named cross-module DI helpers.
// Module A exports a function named `equals` that takes ZERO declared
// parameters and reads `arguments` (so its body and signature differ from
// module B's same-named `equals`). When imported as `{ equals as eqA }`,
// the call must dispatch into THIS body, not module B's.
export function equals(): any {
  return (
    "A:" +
    arguments[0] +
    "," +
    arguments[1] +
    " (len=" +
    arguments.length +
    ")"
  );
}
