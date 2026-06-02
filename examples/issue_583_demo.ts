// Issue #583 demo — deep links (Universal Links / App Links / URL schemes).
//
// Acceptance flow from the issue:
//   1. Configures a custom scheme (`perryapp://…`) and a Universal Link
//      domain (replace `example.com` with your real HTTPS endpoint that
//      serves /.well-known/apple-app-site-association + assetlinks.json).
//   2. The app receives a deep link in three states (cold start / from
//      background / while running) and prints the URL each time.
//
// To exercise on a real device:
//
//   1. Add to package.json (see this file's matching package.json snippet
//      below):
//        "perry": {
//          "deepLinks": {
//            "schemes": ["perryapp"],
//            "universalLinks": {
//              "ios":     ["example.com"],
//              "android": ["example.com"]
//            }
//          }
//        }
//
//   2. Host two server-side files (Perry doesn't host these — it's the
//      app developer's job, per the issue):
//        https://example.com/.well-known/apple-app-site-association
//        https://example.com/.well-known/assetlinks.json
//
//   3. iOS: build + run via `perry run --target ios`, sign with the
//      generated app.entitlements file. Tap a link in Mail / Messages.
//      Android: `perry run --target android`. Tap a link in Gmail.
//
// HEADLESS HOST CAVEAT: a bare CLI binary on macOS doesn't pump the
// AppKit run loop, so the AppDelegate's `application(_:open:)` and the
// kAEGetURL handler never fire. Run inside `App({ body: … })` (or with
// `perry run --target ios-simulator` etc.) for the URL pipeline to be
// active.

import { appOnOpenUrl, appGetLaunchUrl } from "perry/system";

console.log(`[startup] launchUrl=${JSON.stringify(appGetLaunchUrl())}`);

let count = 0;
appOnOpenUrl((url, source) => {
    count++;
    console.log(`[deeplink #${count}] source=${source} url=${url}`);

    // Real apps would route to the relevant screen here. Example:
    //   const u = new URL(url);
    //   if (u.pathname.startsWith("/chat/")) navigateTo("chat", u.pathname);
    //   else if (u.pathname.startsWith("/item/")) navigateTo("item", u.pathname);
});

console.log("[startup] handler installed; tap a deep link to test...");
