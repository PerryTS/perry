// Type declarations for `perry/ads` — Perry's official
// in-app advertising binding (issue #867).
//
// MVP scope: the FFI surface compiles and links across every
// platform crate. Every entry point resolves a structured
// `{ error: "no-sdk-linked" }` placeholder until real SDK
// integration lands. Real Google Mobile Ads SDK integration on
// iOS / Android, GDPR / ATT consent flow, and Info.plist /
// AndroidManifest auto-injection are tracked as follow-ups under
// the same issue.
//
// Configuration (follow-up — not consumed yet) flows from
// `perry.toml`:
//
//     [ads]
//     ios_app_id = "ca-app-pub-XXXX~YYYY"
//     android_app_id = "ca-app-pub-XXXX~ZZZZ"
//     test_device_ids = ["DEVICE_HASH_HERE"]
//     request_non_personalized_ads_only = false
//
// Platform mapping:
//   - iOS / Mac Catalyst: Google Mobile Ads SDK (SwiftPM)
//   - Android: `com.google.android.gms:play-services-ads`
//   - macOS / Linux / Windows / tvOS / watchOS / visionOS / gtk4:
//     no first-party SDK; calls return the same
//     `{ error: "no-sdk-linked" }` shape during MVP, then
//     `{ error: "unsupported-platform" }` once real SDK
//     integration ships on Apple + Android.

/**
 * Result returned by the `*_load` entry points. The runtime
 * never rejects these promises — load failures (no fill, network
 * error, ATT denied, GDPR consent withheld, missing SDK) all flow
 * through the `{ success: false, error }` branch so callers
 * handle every outcome at one `await` site.
 */
export type AdLoadResult =
  | {
      success: true;
    }
  | {
      success: false;
      /**
       * Error slug. The MVP returns `"no-sdk-linked"` on every
       * platform until real SDK integration lands. Real-SDK
       * errors include `"no-fill"`, `"network-error"`,
       * `"consent-required"`, etc.
       */
      error?: string;
    };

/**
 * Result returned by `js_ads_interstitial_show`. `shown` flips
 * once the system actually presented the ad; `dismissed` flips
 * once the user closed it. Both can be false simultaneously
 * (the ad never loaded / the SDK errored mid-present).
 */
export type AdInterstitialShowResult = {
  shown: boolean;
  dismissed: boolean;
  /** Set when the show attempt failed. See MVP slug list above. */
  error?: string;
};

/**
 * Result returned by `js_ads_rewarded_show`. `earned` flips once
 * the SDK's reward callback fires; `amount` and `type` describe
 * the reward (e.g. `{ amount: 50, type: "coins" }`).
 */
export type AdRewardedShowResult = {
  earned: boolean;
  dismissed: boolean;
  /** Reward amount, present only when `earned: true`. */
  amount?: number;
  /** Reward type / currency name, present only when `earned: true`. */
  type?: string;
  /** Set when the show attempt failed. See MVP slug list above. */
  error?: string;
};

/**
 * Preload an interstitial ad against the given AdMob unit ID.
 * Resolves to a JSON-stringified [`AdLoadResult`]. Cache the
 * loaded ad and call [`js_ads_interstitial_show`] when you're
 * ready to present it — typical pattern is "load at start of
 * screen, show at end".
 */
export declare function js_ads_interstitial_load(
  unitId: string,
): Promise<string>;

/**
 * Present the previously-loaded interstitial. Resolves to a
 * JSON-stringified [`AdInterstitialShowResult`] once the user
 * dismisses the ad or the present attempt fails. Calling without
 * a prior successful load resolves with `{ shown: false }`.
 */
export declare function js_ads_interstitial_show(): Promise<string>;

/**
 * Preload a rewarded ad against the given AdMob unit ID.
 * Resolves to a JSON-stringified [`AdLoadResult`]. Pair with
 * [`js_ads_rewarded_show`] when the user opts in.
 */
export declare function js_ads_rewarded_load(unitId: string): Promise<string>;

/**
 * Present the previously-loaded rewarded ad. Resolves to a
 * JSON-stringified [`AdRewardedShowResult`] once the user either
 * earns the reward (`earned: true, amount, type`) or dismisses
 * the ad early (`earned: false, dismissed: true`).
 */
export declare function js_ads_rewarded_show(): Promise<string>;

/**
 * Create a banner widget against the given AdMob unit ID + ad
 * size key. Returns a handle ID that callers pass to
 * [`js_ads_banner_destroy`] when the banner leaves the screen.
 *
 * `sizeKey` must be one of:
 *   - `"banner"`           — 320x50 standard banner
 *   - `"large-banner"`     — 320x100
 *   - `"medium-rectangle"` — 300x250 (IAB MREC)
 *   - `"adaptive"`         — anchored adaptive banner; the SDK
 *                            picks a height based on the device's
 *                            width.
 *
 * v1 exposes the handle directly; perry/ui `<AdBanner>` widget
 * integration so the banner can sit inside the declarative
 * layout tree is a follow-up (#867). The MVP returns `0` since
 * no SDK is linked.
 */
export declare function js_ads_banner_create(
  unitId: string,
  sizeKey: string,
): number;

/**
 * Destroy a banner widget. Callers must invoke this when the
 * banner leaves the screen to release the underlying SDK view
 * and any associated network connections.
 */
export declare function js_ads_banner_destroy(handle: number): void;
