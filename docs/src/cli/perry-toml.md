# perry.toml Reference

`perry.toml` is the project-level configuration file for Perry. It controls project metadata, build settings, platform-specific options, code signing, distribution, auditing, and verification.

Created automatically by `perry init`, it lives at the root of your project alongside `package.json`.

## Minimal Example

```toml
[project]
name = "my-app"
entry = "src/main.ts"

[build]
out_dir = "dist"
```

## Full Example

```toml
[project]
name = "my-app"
version = "1.2.0"
build_number = 42
bundle_id = "com.example.myapp"
description = "A cross-platform Perry application"
entry = "src/main.ts"

[project.icons]
source = "assets/icon.png"

[build]
out_dir = "dist"

[macos]
bundle_id = "com.example.myapp.macos"
category = "public.app-category.developer-tools"
minimum_os = "13.0"
entitlements = ["com.apple.security.network.client"]
distribute = "both"
signing_identity = "Developer ID Application: My Company (TEAMID)"
certificate = "certs/mac-appstore.p12"
notarize_certificate = "certs/mac-devid.p12"
notarize_signing_identity = "Developer ID Application: My Company (TEAMID)"
installer_certificate = "certs/mac-installer.p12"
team_id = "ABCDE12345"
key_id = "KEYID123"
issuer_id = "issuer-uuid-here"
p8_key_path = "certs/AuthKey.p8"
encryption_exempt = true

[ios]
bundle_id = "com.example.myapp.ios"
deployment_target = "16.0"
device_family = ["iphone", "ipad"]
orientations = ["portrait", "landscape-left", "landscape-right"]
capabilities = ["push-notification"]
distribute = "appstore"
entry = "src/main-ios.ts"
provisioning_profile = "certs/MyApp.mobileprovision"
certificate = "certs/ios-distribution.p12"
signing_identity = "iPhone Distribution: My Company (TEAMID)"
team_id = "ABCDE12345"
key_id = "KEYID123"
issuer_id = "issuer-uuid-here"
p8_key_path = "certs/AuthKey.p8"
encryption_exempt = true

[android]
package_name = "com.example.myapp"
min_sdk = "26"
target_sdk = "34"
permissions = ["INTERNET", "CAMERA"]
distribute = "playstore"
keystore = "certs/release.keystore"
key_alias = "my-key"
google_play_key = "certs/play-service-account.json"
entry = "src/main-android.ts"

[linux]
format = "appimage"
category = "Development"
description = "A cross-platform Perry application"

[i18n]
locales = ["en", "de", "fr"]
default_locale = "en"

[i18n.currencies]
en = "USD"
de = "EUR"
fr = "EUR"

[publish]
server = "https://hub.perryts.com"

[audit]
fail_on = "B"
severity = "high"
ignore = ["RULE-001", "RULE-002"]

[verify]
url = "https://verify.perryts.com"
```

---

## Sections

### `[project]`

Core project metadata. This is the primary section for identifying your application.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | Directory name | Project name, used for binary output name and default bundle ID |
| `version` | string | `"1.0.0"` | Semantic version string (e.g., `"1.2.3"`) |
| `build_number` | integer | `1` | Numeric build number; auto-incremented on `perry publish` for iOS, Android, and macOS App Store builds |
| `description` | string | — | Human-readable project description |
| `entry` | string | — | TypeScript entry file (e.g., `"src/main.ts"`). Used by `perry run` and `perry publish` when no input file is specified |
| `bundle_id` | string | `com.perry.<name>` | Default bundle identifier, used as fallback when platform-specific sections don't define one |

#### `[project.icons]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `source` | string | — | Path to a source icon image (PNG or JPG). Perry auto-resizes this to all required sizes for each platform |

### `[app]`

Alternative to `[project]` with identical fields. Useful for organizational clarity — `[app]` takes precedence over `[project]` when both are present:

```toml
# These two are equivalent:
[project]
name = "my-app"

# or:
[app]
name = "my-app"
```

When both exist, resolution order is: `[app]` field -> `[project]` field -> default.

`[app]` supports the same fields as `[project]`: `name`, `version`, `build_number`, `bundle_id`, `description`, `entry`, and `icons`.

---

### `[build]`

Build output settings.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `out_dir` | string | `"dist"` | Directory for build artifacts |

---

### `[macos]`

macOS-specific configuration for `perry publish macos` and `perry compile --target macos`.

#### App Metadata

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `bundle_id` | string | Falls back to `[app]`/`[project]` | macOS-specific bundle identifier (e.g., `"com.example.myapp"`) |
| `category` | string | — | Mac App Store category. Uses Apple's UTI format (see [valid values](#macos-app-store-categories) below) |
| `minimum_os` | string | — | Minimum macOS version required (e.g., `"13.0"`) |
| `entitlements` | string[] | — | macOS entitlements to include in the code signature (e.g., `["com.apple.security.network.client"]`) |
| `encryption_exempt` | bool | `false` | If `true`, adds `ITSAppUsesNonExemptEncryption = false` to Info.plist, skipping the export compliance prompt in App Store Connect |

#### Distribution

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `distribute` | string | — | Distribution method: `"appstore"`, `"notarize"`, or `"both"` (see [Distribution Modes](#macos-distribution-modes)) |

#### Code Signing

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `signing_identity` | string | Auto-detected from Keychain | Code signing identity name (e.g., `"3rd Party Mac Developer Application: Company (TEAMID)"`) |
| `certificate` | string | Auto-exported from Keychain | Path to `.p12` certificate file for App Store distribution |
| `notarize_certificate` | string | — | Separate `.p12` certificate for notarization (only used with `distribute = "both"`) |
| `notarize_signing_identity` | string | — | Signing identity for notarization (only used with `distribute = "both"`) |
| `installer_certificate` | string | — | `.p12` certificate for Mac Installer Distribution (`.pkg` signing) |

#### App Store Connect

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `team_id` | string | From `~/.perry/config.toml` | Apple Developer Team ID |
| `key_id` | string | From `~/.perry/config.toml` | App Store Connect API key ID |
| `issuer_id` | string | From `~/.perry/config.toml` | App Store Connect issuer ID |
| `p8_key_path` | string | From `~/.perry/config.toml` | Path to App Store Connect `.p8` API key file |

#### macOS Distribution Modes

The `distribute` field controls how your macOS app is signed and distributed:

- **`"appstore"`** — Signs with an App Store distribution certificate and uploads to App Store Connect. Requires `team_id`, `key_id`, `issuer_id`, and `p8_key_path`.

- **`"notarize"`** — Signs with a Developer ID certificate and notarizes with Apple. For direct distribution outside the App Store.

- **`"both"`** — Produces **two** signed builds: one for the App Store and one notarized for direct distribution. Requires **two separate certificates**:
  - `certificate` + `signing_identity` for the App Store build
  - `notarize_certificate` + `notarize_signing_identity` for the notarized build
  - Optionally `installer_certificate` for `.pkg` signing

#### macOS App Store Categories

Common values for the `category` field (Apple UTI format):

| Category | Value |
|----------|-------|
| Business | `public.app-category.business` |
| Developer Tools | `public.app-category.developer-tools` |
| Education | `public.app-category.education` |
| Entertainment | `public.app-category.entertainment` |
| Finance | `public.app-category.finance` |
| Games | `public.app-category.games` |
| Graphics & Design | `public.app-category.graphics-design` |
| Health & Fitness | `public.app-category.healthcare-fitness` |
| Lifestyle | `public.app-category.lifestyle` |
| Music | `public.app-category.music` |
| News | `public.app-category.news` |
| Photography | `public.app-category.photography` |
| Productivity | `public.app-category.productivity` |
| Social Networking | `public.app-category.social-networking` |
| Utilities | `public.app-category.utilities` |

---

### `[ios]`

iOS-specific configuration for `perry publish ios`, `perry run ios`, and `perry compile --target ios`/`--target ios-simulator`.

#### App Metadata

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `bundle_id` | string | Falls back to `[app]`/`[project]` | iOS-specific bundle identifier |
| `deployment_target` | string | `"17.0"` | Minimum iOS version required (e.g., `"16.0"`) |
| `minimum_version` | string | — | Alias for `deployment_target` |
| `device_family` | string[] | `["iphone", "ipad"]` | Supported device families |
| `orientations` | string[] | `["portrait"]` | Supported interface orientations |
| `capabilities` | string[] | — | App capabilities (e.g., `["push-notification"]`) |
| `entry` | string | Falls back to `[project]`/`[app]` | iOS-specific entry file (useful when iOS needs a different entry point) |
| `encryption_exempt` | bool | `false` | If `true`, adds `ITSAppUsesNonExemptEncryption = false` to Info.plist |

#### Distribution

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `distribute` | string | — | Distribution method: `"appstore"`, `"testflight"`, or `"development"` |

#### Code Signing

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `signing_identity` | string | Auto-detected from Keychain | Code signing identity (e.g., `"iPhone Distribution: Company (TEAMID)"`) |
| `certificate` | string | Auto-exported from Keychain | Path to `.p12` distribution certificate |
| `provisioning_profile` | string | — | Path to `.mobileprovision` file. Stored as `{bundle_id}.mobileprovision` in `~/.perry/` by `perry setup ios` |

#### App Store Connect

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `team_id` | string | From `~/.perry/config.toml` | Apple Developer Team ID |
| `key_id` | string | From `~/.perry/config.toml` | App Store Connect API key ID |
| `issuer_id` | string | From `~/.perry/config.toml` | App Store Connect issuer ID |
| `p8_key_path` | string | From `~/.perry/config.toml` | Path to `.p8` API key file |

#### Device Family Values

| Value | Description |
|-------|-------------|
| `"iphone"` | iPhone devices |
| `"ipad"` | iPad devices |

#### Orientation Values

| Value | Description |
|-------|-------------|
| `"portrait"` | Device upright |
| `"portrait-upside-down"` | Device upside down |
| `"landscape-left"` | Device rotated left |
| `"landscape-right"` | Device rotated right |

---

### `[visionos]`

visionOS-specific configuration for `perry publish visionos`, `perry run visionos`, and `perry compile --target visionos`/`--target visionos-simulator`.

#### App Metadata

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `bundle_id` | string | Falls back to `[app]`/`[project]`/`[ios]` | visionOS-specific bundle identifier |
| `deployment_target` | string | `"1.0"` | Minimum visionOS version required |
| `minimum_version` | string | — | Alias for `deployment_target` |
| `entry` | string | Falls back to `[project]`/`[app]` | visionOS-specific entry file |
| `encryption_exempt` | bool | `false` | If `true`, adds `ITSAppUsesNonExemptEncryption = false` to Info.plist |
| `info_plist` | table | — | Custom key-value pairs merged into the generated Info.plist |

#### Distribution / Signing

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `distribute` | string | — | Distribution method for visionOS builds |
| `signing_identity` | string | Auto-detected from Keychain | Code signing identity |
| `certificate` | string | Auto-exported from Keychain | Path to `.p12` distribution certificate |
| `provisioning_profile` | string | — | Path to `.mobileprovision` file |
| `team_id` | string | From `~/.perry/config.toml` | Apple Developer Team ID |
| `key_id` | string | From `~/.perry/config.toml` | App Store Connect API key ID |
| `issuer_id` | string | From `~/.perry/config.toml` | App Store Connect issuer ID |
| `p8_key_path` | string | From `~/.perry/config.toml` | Path to `.p8` API key file |

---

### `[android]`

Android-specific configuration for `perry publish android`, `perry run android`, and `perry compile --target android`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `package_name` | string | Falls back to bundle_id chain | Java package name (e.g., `"com.example.myapp"`) |
| `min_sdk` | string | — | Minimum Android SDK version (e.g., `"26"` for Android 8.0) |
| `target_sdk` | string | — | Target Android SDK version (e.g., `"34"` for Android 14) |
| `permissions` | string[] | — | Android permissions (e.g., `["INTERNET", "CAMERA", "ACCESS_FINE_LOCATION"]`) |
| `distribute` | string | — | Distribution method: `"playstore"` |
| `keystore` | string | — | Path to `.jks` or `.keystore` signing keystore |
| `key_alias` | string | — | Alias of the signing key within the keystore |
| `google_play_key` | string | — | Path to Google Play service account JSON file for automated uploads |
| `entry` | string | Falls back to `[project]`/`[app]` | Android-specific entry file |

---

### `[linux]`

Linux-specific configuration for `perry publish linux`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `format` | string | — | Package format: `"appimage"`, `"deb"`, or `"rpm"` |
| `category` | string | — | Desktop application category (e.g., `"Development"`, `"Utility"`, `"Game"`) |
| `description` | string | Falls back to `[project]`/`[app]` | Application description for package metadata |

---

### `[i18n]`

Internationalization configuration. See the [i18n documentation](../i18n/overview.md) for full details.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `locales` | string[] | — | Supported locale codes (e.g., `["en", "de", "fr"]`). Locale files must exist in `/locales` |
| `default_locale` | string | `"en"` | Fallback locale. Used when a key is missing in another locale |
| `dynamic` | boolean | `false` | `false`: locale set at launch, strings inlined. `true`: locale switchable at runtime |

### `[i18n.currencies]`

Maps locale codes to default ISO 4217 currency codes. Used by the `Currency()` format wrapper.

| Key | Type | Description |
|-----|------|-------------|
| `{locale}` | string | Currency code for the locale (e.g., `en = "USD"`, `de = "EUR"`) |

---

### `[publish]`

Publishing configuration.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `server` | string | `https://hub.perryts.com` | Custom Perry Hub build server URL. Useful for self-hosted or enterprise deployments |

---

### `[audit]`

Security audit configuration for `perry audit` and pre-publish audits.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `fail_on` | string | `"C"` | Minimum acceptable audit grade. Build fails if the actual grade is below this threshold. Values: `"A"`, `"A-"`, `"B"`, `"C"`, `"D"`, `"F"` |
| `severity` | string | `"all"` | Filter findings by severity: `"all"`, `"critical"`, `"high"`, `"medium"`, `"low"` |
| `ignore` | string[] | — | List of audit rule IDs to suppress (e.g., `["RULE-001", "RULE-042"]`) |

#### Audit Grade Scale

Grades are ranked from highest to lowest:

| Grade | Rank | Description |
|-------|------|-------------|
| A | 6 | Excellent — no significant findings |
| A- | 5 | Very good — minor findings only |
| B | 4 | Good — some findings |
| C | 3 | Acceptable — moderate findings |
| D | 2 | Poor — significant findings |
| F | 1 | Fail — critical findings |

Setting `fail_on = "B"` means any grade below B (i.e., C, D, or F) will cause the build to fail.

---

### `[verify]`

Runtime verification configuration for `perry verify`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `url` | string | `https://verify.perryts.com` | Verification service endpoint URL |

---

## Bundle ID Resolution

Perry resolves the bundle identifier using a cascading priority system. The first non-empty value wins:

### For iOS builds:
1. `[ios].bundle_id`
2. `[app].bundle_id`
3. `[project].bundle_id`
4. `[macos].bundle_id`
5. `package.json` `bundleId` field
6. `com.perry.<app_name>` (generated default)

### For macOS builds:
1. `[macos].bundle_id`
2. `[app].bundle_id`
3. `[project].bundle_id`
4. `package.json` `bundleId` field
5. `com.perry.<app_name>` (generated default)

### For Android builds:
1. `[android].package_name`
2. `[ios].bundle_id`
3. `[macos].bundle_id`
4. `[app].bundle_id`
5. `[project].bundle_id`
6. `com.perry.<app_name>` (generated default)

---

## Entry File Resolution

When no input file is specified on the command line, Perry resolves the entry file in this order:

1. `[ios].entry` / `[android].entry` (when targeting that platform)
2. `[project].entry` or `[app].entry`
3. `src/main.ts` (if it exists)
4. `main.ts` (if it exists)

---

## Build Number Auto-Increment

The `build_number` field is automatically incremented by `perry publish` for:
- iOS builds
- Android builds
- macOS App Store builds (`distribute = "appstore"` or `"both"`)

The updated value is written back to `perry.toml` after a successful publish. This ensures each submission to the App Store / Play Store has a unique, monotonically increasing build number.

macOS builds with `distribute = "notarize"` (direct distribution) do **not** auto-increment the build number.

---

## Configuration Priority

Perry resolves configuration values using a layered priority system (highest to lowest):

1. **CLI flags** — e.g., `--target`, `--output`
2. **Environment variables** — e.g., `PERRY_LICENSE_KEY`
3. **perry.toml** — project-level config (platform-specific sections first, then `[app]`/`[project]`)
4. **~/.perry/config.toml** — user-level global config
5. **Built-in defaults**

---

## Environment Variables

These environment variables override perry.toml and global config values:

### Apple / iOS / macOS

| Variable | Description |
|----------|-------------|
| `PERRY_LICENSE_KEY` | Perry Hub license key |
| `PERRY_APPLE_CERTIFICATE` | `.p12` certificate file contents (base64) |
| `PERRY_APPLE_CERTIFICATE_PASSWORD` | Password for the `.p12` certificate |
| `PERRY_APPLE_P8_KEY` | `.p8` API key file contents |
| `PERRY_APPLE_KEY_ID` | App Store Connect API key ID |
| `PERRY_APPLE_NOTARIZE_CERTIFICATE_PASSWORD` | Password for the notarization `.p12` certificate |
| `PERRY_APPLE_INSTALLER_CERTIFICATE_PASSWORD` | Password for the installer `.p12` certificate |

### Android

| Variable | Description |
|----------|-------------|
| `PERRY_ANDROID_KEYSTORE` | Path to `.jks`/`.keystore` file |
| `PERRY_ANDROID_KEY_ALIAS` | Keystore key alias |
| `PERRY_ANDROID_KEYSTORE_PASSWORD` | Keystore password |
| `PERRY_ANDROID_KEY_PASSWORD` | Key password (within the keystore) |
| `PERRY_GOOGLE_PLAY_KEY_PATH` | Path to Google Play service account JSON |

### General

| Variable | Description |
|----------|-------------|
| `PERRY_NO_TELEMETRY` | Set to `1` to disable anonymous telemetry |
| `PERRY_NO_UPDATE_CHECK` | Set to `1` to disable background update checks |

---

## Global Config: `~/.perry/config.toml`

Separate from the project-level `perry.toml`, Perry maintains a user-level global config at `~/.perry/config.toml`. This stores credentials and preferences shared across all projects.

```toml
license_key = "perry-xxxxxxxx"
server = "https://hub.perryts.com"
default_target = "macos"

[apple]
team_id = "ABCDE12345"
key_id = "KEYID123"
issuer_id = "issuer-uuid-here"
p8_key_path = "/Users/me/.perry/AuthKey.p8"

[android]
keystore_path = "/Users/me/.perry/release.keystore"
key_alias = "my-key"
google_play_key_path = "/Users/me/.perry/play-service-account.json"
```

Fields in perry.toml (project-level) override `~/.perry/config.toml` (global-level). For example, `[ios].team_id` in perry.toml overrides `[apple].team_id` in the global config.

The global config is managed by `perry setup` commands:
- `perry setup ios` — configures Apple signing credentials
- `perry setup android` — configures Android signing credentials
- `perry setup macos` — configures macOS distribution settings

---

## perry.toml vs package.json

Perry reads configuration from both files. Here's what goes where:

| Setting | File | Section |
|---------|------|---------|
| Compile packages natively | `package.json` | `perry.compilePackages` |
| Splash screen | `package.json` | `perry.splash` |
| Project name, version, entry | `perry.toml` | `[project]` |
| Platform-specific settings | `perry.toml` | `[ios]`, `[macos]`, `[android]`, `[linux]` |
| Code signing & distribution | `perry.toml` | Platform sections |
| Build output directory | `perry.toml` | `[build]` |
| Audit & verification | `perry.toml` | `[audit]`, `[verify]` |

When both files define the same value (e.g., project name), `perry.toml` takes precedence.

---

## Setup Wizard

Running `perry setup <platform>` interactively configures signing credentials and writes them back to both `perry.toml` and `~/.perry/config.toml`:

```bash
perry setup ios       # Configure iOS signing (certificate, provisioning profile)
perry setup android   # Configure Android signing (keystore, Play Store key)
perry setup macos     # Configure macOS distribution (App Store, notarization)
```

The wizard automatically:
- Sets `[ios].distribute = "testflight"` if not already configured
- Sets `[android].distribute = "playstore"` if not already configured
- Stores provisioning profiles as `~/.perry/{bundle_id}.mobileprovision`
- Auto-exports `.p12` certificates from macOS Keychain when possible

---

## CI/CD Example

For CI environments, use environment variables instead of storing credentials in `perry.toml`:

```yaml
# GitHub Actions example
env:
  PERRY_LICENSE_KEY: ${{ secrets.PERRY_LICENSE_KEY }}
  PERRY_APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
  PERRY_APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERT_PASSWORD }}
  PERRY_APPLE_P8_KEY: ${{ secrets.APPLE_P8_KEY }}
  PERRY_APPLE_KEY_ID: ${{ secrets.APPLE_KEY_ID }}
  PERRY_ANDROID_KEYSTORE: ${{ secrets.ANDROID_KEYSTORE }}
  PERRY_ANDROID_KEYSTORE_PASSWORD: ${{ secrets.ANDROID_KEYSTORE_PASSWORD }}
  PERRY_ANDROID_KEY_ALIAS: ${{ secrets.ANDROID_KEY_ALIAS }}
  PERRY_ANDROID_KEY_PASSWORD: ${{ secrets.ANDROID_KEY_PASSWORD }}

steps:
  - run: perry publish ios
  - run: perry publish android
  - run: perry publish macos
```

Keep `perry.toml` in version control with non-sensitive fields only:

```toml
[project]
name = "my-app"
version = "2.1.0"
build_number = 47
bundle_id = "com.example.myapp"
entry = "src/main.ts"

[ios]
deployment_target = "16.0"
device_family = ["iphone", "ipad"]
distribute = "appstore"
encryption_exempt = true

[android]
package_name = "com.example.myapp"
min_sdk = "26"
target_sdk = "34"
distribute = "playstore"

[macos]
distribute = "both"
category = "public.app-category.productivity"
minimum_os = "13.0"

[audit]
fail_on = "B"
```
