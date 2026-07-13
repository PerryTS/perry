// Issue #6087 — a user function imported from a plain TypeScript module must
// never be hijacked by the name-keyed perry/system | perry/updater |
// perry/background dispatch tables.
//
// Pre-fix, `lower_call/extern_func.rs` consulted those tables on the bare
// callee name with no check of where the name came from. Since an imported
// function lowers to `Expr::ExternFuncRef { name }` — exactly like a
// `perry/system` import does — every user function whose name collided with
// one of the ~60 table rows was routed into the native table:
//
//   * arity != the table row  -> `lower_perry_ui_table_call` lowered the args
//     for side effects and returned the 0.0 sentinel, i.e. THE CALL VANISHED.
//     `takeScreenshot("a.png")` printed nothing, with no error and no warning.
//   * arity == the table row  -> the program linked against an undefined
//     `perry_system_haptic_play` symbol despite importing nothing native.
//
// The fix gates all three tables on the callee actually resolving to a
// `perry/system` / `perry/updater` / `perry/background` import.
//
// This file imports NOTHING native. Every call below must reach the user's
// function in ./_helpers/perry_builtin_name_collision_lib.ts.

import {
  takeScreenshot,
  hapticPlay,
  openURL,
  getLocale,
  isDarkMode,
  preferencesSet,
  cancel,
  schedule,
  compareVersions,
  relaunch,
} from "./_helpers/perry_builtin_name_collision_lib.ts";

// perry/system rows
console.log(takeScreenshot("a.png"));
console.log(hapticPlay("light"));
console.log(openURL("https://example.com"));
console.log(getLocale());
console.log(isDarkMode());
console.log(preferencesSet("k", "v", 7));

// perry/background rows
console.log(cancel("job-1"));
console.log(schedule("job-2", 30, true));

// perry/updater rows
console.log(compareVersions("1.2.0", "1.10.0"));
console.log(relaunch());

// The collision must not survive indirection either: a call through a local
// alias, and a call from inside another function, both lower to the same
// ExternFuncRef.
const aliased = takeScreenshot;
console.log(aliased("b.png"));

function nested(): void {
  console.log(takeScreenshot("c.png"));
  console.log(cancel("job-3"));
}
nested();

console.log("done");
