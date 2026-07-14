# Installation

## Prerequisites

Perry compiles TypeScript to native binaries by linking with your system's C toolchain, so every install path needs a linker:

- **macOS**: Xcode Command Line Tools (`xcode-select --install`)
- **Linux**: `gcc` or `clang` for linking, plus **clang ≥ 15** for codegen — see your distro below
- **Windows**: LLVM (`winget install LLVM.LLVM`) + `perry setup windows` (lightweight, ~1.5 GB, no Visual Studio needed), or MSVC Build Tools with the "Desktop development with C++" workload — see the [Windows platform guide](../platforms/windows.md) for both options

> **clang ≥ 15 on Linux.** Perry's LLVM backend emits opaque-pointer IR (`ptr`) and compiles it with `clang -c`. clang 14 and older reject it with `error: expected type`. Ubuntu 22.04's default `clang` is 14 — install a newer one (`sudo apt install clang-15`) and point Perry at it if it isn't the default: `export PERRY_LLVM_CLANG=/usr/bin/clang-15`. Ubuntu 24.04, Debian 13, Fedora 39+ and Arch all ship a new enough clang.

Linux C toolchain by distribution:

```bash
# Debian / Ubuntu / Pop!_OS / Mint
sudo apt install build-essential

# Arch / Manjaro / CachyOS / EndeavourOS
sudo pacman -S base-devel gcc

# Fedora / RHEL / CentOS Stream
sudo dnf install gcc gcc-c++ glibc-devel

# openSUSE
sudo zypper install -t pattern devel_basis

# Alpine / musl-based
sudo apk add build-base

# Void Linux
sudo xbps-install -S base-devel
```

The source install additionally needs the **Rust toolchain** via [rustup](https://rustup.rs/).

## Install Perry

### npm / npx (recommended — any platform)

Perry ships as a prebuilt-binary npm package. This is the fastest way to get started and the only path that covers all seven supported platforms (macOS arm64/x64, Linux x64/arm64 glibc + musl, Windows x64) with a single command:

```bash
# Project-local (pins Perry's version alongside your deps)
npm install @perryts/perry
npx perry compile src/main.ts -o myapp && ./myapp

# Global
npm install -g @perryts/perry
perry compile src/main.ts -o myapp

# Zero-install, one-shot
npx -y @perryts/perry compile src/main.ts -o myapp
```

[`@perryts/perry`](https://www.npmjs.com/package/@perryts/perry) is a thin launcher; npm automatically picks the matching prebuilt via `optionalDependencies` (`@perryts/perry-darwin-arm64`, `@perryts/perry-linux-x64-musl`, etc.) based on your `os` / `cpu` / `libc`. Requires Node.js ≥ 16.

| Platform | Prebuilt package |
|---|---|
| macOS arm64 (Apple Silicon) | `@perryts/perry-darwin-arm64` |
| macOS x64 (Intel) | `@perryts/perry-darwin-x64` |
| Linux x64 (glibc) | `@perryts/perry-linux-x64` |
| Linux arm64 (glibc) | `@perryts/perry-linux-arm64` |
| Linux x64 (musl / Alpine) | `@perryts/perry-linux-x64-musl` |
| Linux arm64 (musl / Alpine) | `@perryts/perry-linux-arm64-musl` |
| Windows x64 | `@perryts/perry-win32-x64` |

#### Linux glibc requirement

The Linux **glibc** binaries are built on Ubuntu 24.04, so they require **glibc ≥ 2.39**. Older distributions — Ubuntu 22.04 (glibc 2.35), Debian 12 (2.36), RHEL 9 / Amazon Linux 2023 (2.34) — cannot load them; the dynamic loader fails with `GLIBC_2.39 not found` before Perry starts.

On those hosts Perry uses the **fully-static musl build** instead, which has no libc dependency and runs on any Linux:

- **`install.sh`** detects the glibc version and downloads `perry-linux-<arch>-musl.tar.gz` automatically.
- **npm** — the launcher routes to `@perryts/perry-linux-x64-musl` (or `-arm64-musl`) and prints a one-time notice. npm does not install that package on a glibc machine by itself (its `libc` selector says `musl`), so install it once:

  ```bash
  npm install --force @perryts/perry-linux-x64-musl
  ```

The static build is the same compiler and produces the same binaries. The only feature it does not support is `perry/ui` (GTK4 desktop apps), which needs glibc. Tracking issue: [#6298](https://github.com/PerryTS/perry/issues/6298).

### Homebrew (macOS)

```bash
brew install perryts/perry/perry
```

### winget (Windows)

```bash
winget install PerryTS.Perry
```

### APT (Debian / Ubuntu)

```bash
curl -fsSL https://perryts.github.io/perry-apt/perry.gpg.pub | sudo gpg --dearmor -o /usr/share/keyrings/perry.gpg
echo "deb [signed-by=/usr/share/keyrings/perry.gpg] https://perryts.github.io/perry-apt stable main" | sudo tee /etc/apt/sources.list.d/perry.list
sudo apt update && sudo apt install perry
```

### From Source

```bash
git clone https://github.com/PerryTS/perry.git
cd perry
cargo build --release
```

The binary is at `target/release/perry`. Add it to your PATH:

```bash
# Add to ~/.zshrc or ~/.bashrc
export PATH="/path/to/perry/target/release:$PATH"
```

### Self-Update

Once installed, Perry can update itself:

```bash
perry update
```

This downloads the latest release and atomically replaces the binary.

## Verify Installation

```bash
perry doctor
```

This checks your installation, shows the current version, and reports if an update is available.

```bash
perry --version
```

## Platform-Specific Setup

### macOS

No additional setup needed. Perry uses the system `cc` linker and AppKit for UI apps.

For iOS development, install Xcode (not just Command Line Tools) for the iOS SDK and simulator.

### Linux

Install GTK4 + libshumate + GStreamer development libraries for UI apps. (You
only need these if you build for `--target linux` — pure-CLI / cross-compile
to other platforms doesn't require them.)

```bash
# Ubuntu / Debian
sudo apt install libgtk-4-dev libshumate-dev libgstreamer1.0-dev

# Fedora
sudo dnf install gtk4-devel libshumate-devel gstreamer1-devel \
                 gstreamer1-plugins-base-devel

# Arch
sudo pacman -S gtk4 libshumate gstreamer gst-plugins-base
```

### Windows

Two toolchain options — pick one. Both produce identical binaries.

**Lightweight (recommended, ~1.5 GB, no Visual Studio):**

```powershell
winget install LLVM.LLVM
perry setup windows
```

`perry setup windows` downloads the Microsoft CRT + Windows SDK libraries via xwin after prompting for license acceptance. Pass `--accept-license` to skip the prompt in CI.

**MSVC Build Tools (~8 GB):**

Install Visual Studio Build Tools with the "Desktop development with C++" workload — via the Visual Studio Installer, or:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override `
  "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Run `perry doctor` to verify the toolchain. See the [Windows platform guide](../platforms/windows.md) for details.

## What's Next

- [Write your first program](hello-world.md)
- [Build a native app](first-app.md)

### Authenticated self-update and release migration

`perry update` only installs a release when the matching
`<archive>.update.json` is present and validates against a public-key keyring
compiled into the CLI. The manifest is Ed25519-signed over a domain-separated
payload that binds the key id, version, platform, artifact name, HTTPS URL,
SHA-256 digest, and size. Old releases that only publish `*.sha256` sidecars
are therefore intentionally **not** eligible for automatic installation;
download them manually from the release page.

Release maintainers must configure these GitHub settings before enabling a
release: repository variable `PERRY_CLI_UPDATE_PUBLIC_KEYS` (a JSON array of
`{"key_id":"...","public_key":"<base64-32-byte-Ed25519-key>"}`),
repository variable `PERRY_CLI_UPDATE_KEY_ID`, and protected secret
`PERRY_CLI_UPDATE_SIGNING_KEY` (the matching base64 32-byte seed). The workflow
fails rather than publishing an unsigned manifest when the secret/key id is
missing. Keep the old public key in the compiled keyring during rotation, sign
new manifests with the new `key_id`, and remove the old key only after the
minimum supported CLI has shipped with the replacement.

The updater stages under the install directory with owner-only permissions,
verifies before extraction, rejects links/path traversal, and restores the old
binary and libraries after an interrupted transaction. It never recommends
running the updater with elevated privileges; use the package manager or a
manual install when the installation directory is not writable.
