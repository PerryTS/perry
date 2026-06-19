//! Bundled trust allowlist for lifecycle-script execution.
//!
//! v1 ships allowlist-only script execution (no sandbox). The list
//! covers the well-known native-binding / build-step packages whose
//! `postinstall` is essential for the package to work (e.g. esbuild
//! downloads its platform-specific binary; prisma generates client
//! code; sharp builds its native module).
//!
//! v2 will sandbox these (macOS sandbox-exec / Linux bubblewrap +
//! seccomp / Windows AppContainer) so even a compromised allowlisted
//! package's script can't exfil credentials. Until v2 lands, the
//! allowlist is the trust boundary: a malicious version of an
//! allowlisted package WOULD run, so users on high-stakes hosts can
//! `--installer=npm` + omit the allowlist via package.json's
//! `perry.disallowScripts` (TODO Phase 9.1) or simply
//! `--run-scripts-all=false` (the default).

const EXACT: &[&str] = &[
    // Build / bundler tooling
    "esbuild",
    "swc",
    "@swc/core",
    "@swc/wasm",
    "@biomejs/biome",
    "lightningcss",
    // Database / ORM
    "prisma",
    "@prisma/client",
    "@prisma/engines",
    "better-sqlite3",
    "sqlite3",
    "leveldown",
    // Crypto / native compute
    "bcrypt",
    "argon2",
    "node-sass", // legacy but still in long-tail use
    // Image / media
    "sharp",
    "canvas",
    // Browser automation
    "electron",
    "puppeteer",
    "puppeteer-core",
    "playwright",
    "@playwright/test",
    "@playwright/browser-chromium",
    // WebSocket native
    "bufferutil",
    "utf-8-validate",
    // macOS-only fs watcher
    "fsevents",
    // Native binding glue
    "node-gyp",
    "node-gyp-build",
    "node-pre-gyp",
    "prebuild-install",
    "@mapbox/node-pre-gyp",
    "@parcel/watcher",
    "protobufjs",
    "grpc",
    "@grpc/grpc-js",
];

/// Scoped prefixes — match any package whose name starts with one of
/// these. Used for platform-specific subpackages that npm ships per
/// (os, arch) and that need their install script (a platform check, a
/// binary chmod, etc.). Listing each variant explicitly would be
/// tedious and version-fragile.
const PREFIXES: &[&str] = &[
    "@next/swc-",
    "@tailwindcss/oxide",
    "@biomejs/cli-",
    "@esbuild/",
    "@swc/core-",
    "lightningcss-",
    "@rollup/rollup-",
    "@parcel/",
    "@playwright/",
    "@puppeteer/",
    "@prisma/",
    "@napi-rs/",
];

/// Is this package on the bundled trust allowlist?
pub fn is_bundled(name: &str) -> bool {
    EXACT.contains(&name) || PREFIXES.iter().any(|p| name.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_matches() {
        assert!(is_bundled("esbuild"));
        assert!(is_bundled("sharp"));
        assert!(is_bundled("prisma"));
        assert!(is_bundled("@prisma/client"));
    }

    #[test]
    fn prefix_matches() {
        assert!(is_bundled("@next/swc-darwin-arm64"));
        assert!(is_bundled("@esbuild/darwin-arm64"));
        assert!(is_bundled("@tailwindcss/oxide-linux-x64-gnu"));
        assert!(is_bundled("@napi-rs/canvas-darwin-arm64"));
    }

    #[test]
    fn unrelated_not_matched() {
        assert!(!is_bundled("lodash"));
        assert!(!is_bundled("evilpkg"));
        assert!(!is_bundled("esbuild-evil")); // typosquat shape, not allowed
        assert!(!is_bundled("@scope/random"));
    }
}
