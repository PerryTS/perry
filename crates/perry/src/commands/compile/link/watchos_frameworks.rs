//! watchOS framework link flags.
//!
//! Split out of `build_and_run.rs` (2000-line-per-file cap). Pure
//! relocation of the `if is_watchos { ... }` arm body.

/// Append the watchOS framework arguments for this build.
pub(super) fn append_watchos_frameworks(
    cmd: &mut std::process::Command,
    compiled_features: &[String],
) {
    // watchOS frameworks (swiftc auto-links Swift stdlib on the non-game-loop path)
    let is_watchos_game_loop = compiled_features.iter().any(|f| f == "watchos-game-loop");
    let is_watchos_swift_app = compiled_features.iter().any(|f| f == "watchos-swift-app");
    if !is_watchos_game_loop {
        cmd.arg("-framework").arg("SwiftUI");
    }
    cmd.arg("-framework")
        .arg("WatchKit")
        .arg("-framework")
        .arg("Foundation")
        .arg("-framework")
        .arg("CoreFoundation")
        .arg("-framework")
        .arg("Security")
        .arg("-framework")
        .arg("UserNotifications") // UNUserNotificationCenter (perry/system notificationSend/Schedule/OnTap)
        // AVFAudio: AVAudioEngine / AVAudioSession / AVAudioApplication for
        // microphone capture + the record-permission API (perry/system
        // audioStart, getLevel, recording). Without this the audio classes
        // aren't registered in the objc runtime, so `AnyClass::get` returns
        // nil and audio silently no-ops on device — e.g. a watchOS dB meter
        // shows no levels and never prompts for mic permission. (The iOS
        // branch already links these; watchOS was missing them.)
        .arg("-framework")
        .arg("AVFAudio")
        .arg("-framework")
        .arg("AVFoundation")
        .arg("-lSystem")
        .arg("-lresolv");
    if is_watchos_game_loop {
        // QuartzCore for CAMetalLayer-backed rendering (Metal.framework is NOT
        // in the watchOS SDK — the native lib must dlopen it or supply its own
        // path to the device's Metal dylib). -lobjc for the dynamic
        // WKApplicationDelegate class registered from watchos_game_loop.rs.
        cmd.arg("-framework").arg("QuartzCore").arg("-lobjc");
    }
    if is_watchos_swift_app {
        // SceneKit for SceneView-backed 3D rendering from the native lib's
        // `@main struct App: App`. The lib may additionally use Canvas (2D,
        // already covered by SwiftUI) or SpriteKit (opt-in via the
        // manifest's `frameworks` list).
        cmd.arg("-framework").arg("SceneKit");
    }
}
