// #867 — compile-smoke for the perry/ui AdBanner widget.
//
// Verifies the AdBanner dispatch row + d.ts + per-platform
// `perry_ui_adbanner_create` FFI link end-to-end. On macOS the banner is
// a layout placeholder (no macOS Ads SDK), so this builds and lays out
// without showing a real ad. UI smoke tests are skip-listed from the
// headless compile-smoke job (they open a window); this is here for the
// local `perry test_ui_adbanner_smoke.ts -o out` build check.
import { App, VStack, Text, AdBanner } from "perry/ui";

App({
  title: "AdBanner smoke",
  body: VStack(8, [
    Text("Free app with a banner ad"),
    AdBanner("ca-app-pub-3940256099942544/2934735716", "banner"),
  ]),
});
