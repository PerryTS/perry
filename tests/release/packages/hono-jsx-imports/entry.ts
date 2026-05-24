// Tier-3 fixture (#1671): the `hono/jsx` + `hono/jsx/streaming` module
// imports must resolve and LINK under Perry-native compile.
//
// `renderToReadableStream` lives in hono/jsx/streaming, which transitively
// imports hono/jsx/dom/render → hono/jsx/hooks. hooks/index.js schedules a
// re-render via a single-argument `setTimeout(() => { … })`. Before #1671,
// codegen had no extern-func arm for 1-arg setTimeout, so the call fell
// through to a bare `@setTimeout` LLVM call and the binary failed to link
// with `Undefined symbols: _setTimeout`. This fixture pins the link path.
//
// Imports only (no JSX syntax) so `node --experimental-strip-types` — which
// strips types but does NOT transform JSX — can run it to keep expected.txt
// honest against the pinned hono version.
import { renderToReadableStream, Suspense } from "hono/jsx/streaming";
import { jsx, Fragment } from "hono/jsx";

console.log("renderToReadableStream:", typeof renderToReadableStream);
console.log("Suspense:", typeof Suspense);
console.log("jsx:", typeof jsx);
console.log("Fragment:", typeof Fragment);
