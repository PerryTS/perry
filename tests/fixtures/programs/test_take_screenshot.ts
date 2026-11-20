// Smoke test for perry/system takeScreenshot() (issue #918).
//
// In CLI builds with no UI host attached, takeScreenshot() returns an
// empty string. This test just confirms the function:
//   - is callable from TypeScript (i.e. compiles and links),
//   - returns a string (so typeof === "string"),
//   - does not crash the runtime.
//
// On a UI-hosted build (macOS/iOS/tvOS/visionOS/GTK4/Windows/Android) the
// returned string would be a base64-encoded PNG of the key window contents;
// validating that requires a UI host so it's not exercised here.

import { takeScreenshot } from "perry/system";

const png = takeScreenshot();
console.log("typeof:", typeof png);
console.log("length:", png.length);
console.log("ok");
