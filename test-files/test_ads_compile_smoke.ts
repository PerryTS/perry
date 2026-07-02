// Compile-smoke for `perry/ads` (issue #867).
//
// References each of the seven FFI entry points and prints the
// result. On the build host (macOS native) the promise-returning
// entries resolve a structured `{ error: "unsupported-platform" }`
// shape (no Google Mobile Ads SDK on macOS) and banner_create returns
// 0, so the program exits 0 with the JSON lines + 1 numeric handle.

import {
  js_ads_interstitial_load,
  js_ads_interstitial_show,
  js_ads_rewarded_load,
  js_ads_rewarded_show,
  js_ads_banner_create,
  js_ads_banner_destroy,
  js_ads_request_consent,
} from "perry/ads";

async function main() {
  const a = await js_ads_interstitial_load("ca-app-pub-test/interstitial");
  console.log("interstitial_load:", a);

  const b = await js_ads_interstitial_show();
  console.log("interstitial_show:", b);

  const c = await js_ads_rewarded_load("ca-app-pub-test/rewarded");
  console.log("rewarded_load:", c);

  const d = await js_ads_rewarded_show();
  console.log("rewarded_show:", d);

  const handle = js_ads_banner_create("ca-app-pub-test/banner", "banner");
  console.log("banner_create:", handle);

  js_ads_banner_destroy(handle);
  console.log("banner_destroy: ok");

  const consent = await js_ads_request_consent();
  console.log("request_consent:", consent);
}

main();
