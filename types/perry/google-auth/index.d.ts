// Type declarations for `@perryts/google-auth` — Perry's official
// Google Sign In binding (issue #674).
//
// Configuration flows from `perry.toml`:
//
//     [google_auth]
//     ios_client_id = "..."
//     android_client_id = "..."
//     server_client_id = "..."         # for backend ID-token verification
//     default_scopes = ["openid", "email", "profile"]
//
// Platform mapping:
//   - iOS / Mac Catalyst / macOS 13+: GoogleSignIn SDK (SwiftPM)
//   - Android: androidx.credentials Credential Manager
//                + GetGoogleIdOption
//   - Linux / Windows: stub (system-browser + loopback OAuth is a
//                      follow-up, see #674)
//   - tvOS / watchOS / visionOS / gtk4: no-op stub

/**
 * The shape of the JSON string each `js_google_auth_*` promise
 * resolves to. Two variants; discriminated by the `success` field.
 *
 * The runtime never rejects these promises — cancellation, missing
 * cached credentials, and SDK-level errors all come back through
 * the `{ success: false, ... }` shape so callers handle every
 * outcome at one site.
 */
export type GoogleSignInResult =
  | {
      success: true;
      /** ID token (JWT) — verify against `server_client_id` server-side. */
      idToken: string;
      /** OAuth access token; only present when the user granted at least one scope that requires it. */
      accessToken?: string;
      /** Stable Google user id (`sub`). */
      userId: string;
      email: string;
      emailVerified: boolean;
      name?: string;
      pictureUrl?: string;
      /** Scopes the user actually granted (intersection of requested + approved). */
      grantedScopes: string[];
    }
  | {
      success: false;
      /** True when the user dismissed the system sign-in sheet. */
      cancelled?: boolean;
      /** Error slug. The MVP returns `"not-yet-implemented"` on
       *  iOS/macOS/Android (SDK integration is post-MVP work) and
       *  `"unsupported-platform"` on Linux/Windows/tvOS/watchOS/
       *  visionOS/gtk4. Real SDK errors land here once integration
       *  lands. */
      error?: string;
    };

/**
 * Interactive Google Sign In. Presents the system sign-in sheet
 * on iOS/macOS, dispatches `CredentialManager.getCredential` with
 * `GetGoogleIdOption` on Android. Resolves to a JSON-stringified
 * [`GoogleSignInResult`].
 */
export declare function js_google_auth_sign_in(): Promise<string>;

/**
 * Restore a previous sign-in without user interaction. Resolves
 * to the same shape as [`js_google_auth_sign_in`]; reports
 * `success: false` (no `cancelled` flag) when no cached session
 * exists or the refresh failed.
 */
export declare function js_google_auth_silent_sign_in(): Promise<string>;

/**
 * Clear cached credentials. Resolves to a JSON-stringified
 * `GoogleSignInResult` with `success: true` on platforms where
 * the SDK reports a successful sign-out, otherwise
 * `{ success: false, error }`.
 */
export declare function js_google_auth_sign_out(): Promise<string>;
