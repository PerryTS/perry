//! macOS system libraries and frameworks a Perry program links. Shared by
//! Perry's own link and the staticlib manifest (`staticlib`).

use std::process::Command;

use super::CompilationContext;

/// Frameworks for the stdlib (rustls platform verifier / keychain, DNS,
/// iconv), UI, plugins, and geisterhand.
pub(super) fn add_system_libs(cmd: &mut Command, ctx: &CompilationContext) {
    // Runtime-only console binaries import exclusively from libSystem
    // (verified via `dyld_info -imports` on a dead-stripped build), and
    // every extra LC_LOAD_DYLIB costs real launch time — measured on an
    // M-series host, CoreFoundation+libobjc initialization alone is
    // ~+1.8 ms on a ~3 ms hello world. Skip the whole list for those
    // links: a runtime-only program that genuinely needs one of these
    // fails AT LINK TIME with an undefined symbol (deterministic,
    // caught by the parity suites), never at runtime.
    if ctx.needs_stdlib || ctx.needs_ui || ctx.needs_plugins || ctx.needs_geisterhand {
        for framework in ["Security", "CoreFoundation", "SystemConfiguration"] {
            cmd.arg("-framework").arg(framework);
        }
        cmd.args(["-liconv", "-lresolv", "-lobjc"]);
    }
    // #8513: notify's macOS backend references FSEventStream*. Cargo
    // normally propagates fsevent-sys's CoreServices link metadata to a
    // final Rust binary, but a perry-ext static archive crosses Perry's
    // custom linker boundary, where that metadata is no longer present.
    // Restore it only for binaries that selected the watcher facade.
    if ctx
        .native_module_imports
        .iter()
        .any(|module| module == "@parcel/watcher" || module.starts_with("@parcel/watcher-"))
    {
        cmd.arg("-framework").arg("CoreServices");
    }
}

pub(super) fn add_ui_frameworks(cmd: &mut Command) {
    for framework in [
        "AppKit",
        // perry/ui WebView (#658) — WKWebView / WKWebViewConfiguration.
        "WebKit",
        "CoreGraphics",
        "QuartzCore",
        "AVFoundation",
        "Metal",
        "IOKit",
        "DiskArbitration", // needed by CoreGraphics
        // perry/media — AVPlayer is in AVFoundation (already linked
        // above). CoreMedia provides CMTime + CMTimeGetSeconds /
        // CMTimeMakeWithSeconds used for seek + position. MediaPlayer
        // provides MPNowPlayingInfoCenter / MPRemoteCommandCenter /
        // MPMediaItemArtwork (lock screen + Touch Bar + Now Playing).
        "CoreMedia",
        "MediaPlayer",
        // perry/ui MapView (#517) — MKMapView lives in MapKit.
        "MapKit",
        // perry/ui PdfView (#516) — PDFView lives in PDFKit, which
        // also exposes the PDFDocument / PDFPage classes used for
        // page-count + page-navigation queries.
        "PDFKit",
        // perry/system network reachability (#582) — NWPathMonitor.
        "Network",
    ] {
        cmd.arg("-framework").arg(framework);
    }
}
