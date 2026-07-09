//! Linux GTK4 UI system libraries for the link line.
//!
//! Extracted from `build_and_run.rs` (file-size gate), where the same
//! pkg-config → warn → hardcoded-fallback shape was stamped out four times:
//! GTK4 (#181), GStreamer (#423), libshumate (#517), WebKitGTK (#658). The
//! fallback fires in two distinct cases: pkg-config not installed (spawn
//! fails), OR installed but the `.pc` file not on the search path (exit != 0
//! — happens e.g. on Ubuntu hosts where a dev package is split, or when
//! PKG_CONFIG_PATH is locked down). Pre-#181 the second case silently
//! emitted no GTK link flags and the link bombed with hundreds of
//! `g_object_unref` / `gtk_widget_*` undefined references.

use std::process::Command;

/// One pkg-config-resolved library group with a hardcoded fallback.
struct PkgConfigLibGroup {
    /// Package names passed to `pkg-config --libs …`.
    packages: &'static [&'static str],
    /// Human name used in the fallback warning ("GTK4", "GStreamer", …).
    what: &'static str,
    /// `-l…` flags emitted when pkg-config yields nothing.
    fallback_libs: &'static [&'static str],
    /// Install advice appended to the fallback warning.
    advice: &'static str,
}

/// Mirrors what `pkg-config --libs gtk4` returns on a standard libgtk-4-dev
/// install. Pre-#181 the fallback only listed the glib/gio core, which left
/// pango/cairo/gdk_pixbuf undefined.
const GTK4: PkgConfigLibGroup = PkgConfigLibGroup {
    packages: &["gtk4"],
    what: "GTK4",
    fallback_libs: &[
        "-lgtk-4",
        "-lgio-2.0",
        "-lgobject-2.0",
        "-lglib-2.0",
        "-lpangocairo-1.0",
        "-lpango-1.0",
        "-lharfbuzz",
        "-lgdk_pixbuf-2.0",
        "-lcairo-gobject",
        "-lcairo",
        "-lgraphene-1.0",
    ],
    advice: "install `libgtk-4-dev` (Debian/Ubuntu) or `gtk4-devel` (Fedora/RHEL) \
             and ensure pkg-config can find `gtk4.pc` to silence this warning.",
};

/// GStreamer libs — pulled in by perry-ui-gtk4's gstreamer-rs dep (added in
/// v0.5.440 for the perry/media playbin backend). GTK4's pkg-config doesn't
/// transitively reference the gstreamer-1.0 sonames, so `-lgstreamer-1.0`
/// (and the base/app/video/audio sublibs that gstreamer-rs's playbin path
/// touches) have to land on the link line explicitly or ld fails with
/// `undefined reference to gst_message_parse_buffering` + `DSO missing from
/// command line` (#423).
const GSTREAMER: PkgConfigLibGroup = PkgConfigLibGroup {
    packages: &[
        "gstreamer-1.0",
        "gstreamer-base-1.0",
        "gstreamer-app-1.0",
        "gstreamer-video-1.0",
        "gstreamer-audio-1.0",
    ],
    what: "GStreamer",
    fallback_libs: &[
        "-lgstreamer-1.0",
        "-lgstbase-1.0",
        "-lgstapp-1.0",
        "-lgstvideo-1.0",
        "-lgstaudio-1.0",
    ],
    advice: "install `libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev` \
             (Debian/Ubuntu) or `gstreamer1-devel gstreamer1-plugins-base-devel` \
             (Fedora/RHEL) to silence this warning.",
};

/// libshumate — GNOME's GTK4 vector-tile map widget for the perry/ui
/// MapView (#517).
const SHUMATE: PkgConfigLibGroup = PkgConfigLibGroup {
    packages: &["shumate-1.0"],
    what: "libshumate",
    fallback_libs: &["-lshumate-1.0"],
    advice: "install `libshumate-dev` (Debian/Ubuntu) or `libshumate-devel` \
             (Fedora/RHEL) to silence this warning.",
};

/// WebKitGTK 6.0 + libsoup-3.0 — perry/ui WebView (#658, v0.5.864).
/// perry-ui-gtk4's webkit6/soup3 deps reference symbols like
/// `soup_check_version` from libsoup-3.0 transitively; without explicit
/// `-lsoup-3.0` ld errors with `DSO missing from command line`.
const WEBKITGTK: PkgConfigLibGroup = PkgConfigLibGroup {
    packages: &["webkitgtk-6.0", "libsoup-3.0"],
    what: "WebKitGTK",
    fallback_libs: &["-lwebkitgtk-6.0", "-ljavascriptcoregtk-6.0", "-lsoup-3.0"],
    advice: "install `libwebkitgtk-6.0-dev` (Debian/Ubuntu) which pulls \
             libsoup-3.0-dev + libjavascriptcoregtk-6.0-dev to silence this \
             warning.",
};

/// Append the system libraries the GTK4 UI backend needs: GTK4, PulseAudio
/// (audio capture — soname-stable, no pkg-config group needed), GStreamer,
/// libshumate, and WebKitGTK, in that order. Each pkg-config group falls
/// back to its hardcoded link set with a warning naming the dev package to
/// install.
pub(super) fn add_linux_ui_system_libs(cmd: &mut Command) {
    add_pkg_config_libs(cmd, &GTK4);
    // PulseAudio for audio capture (only needed with UI)
    cmd.arg("-lpulse-simple").arg("-lpulse");
    add_pkg_config_libs(cmd, &GSTREAMER);
    add_pkg_config_libs(cmd, &SHUMATE);
    add_pkg_config_libs(cmd, &WEBKITGTK);
}

fn add_pkg_config_libs(cmd: &mut Command, group: &PkgConfigLibGroup) {
    let mut pc = Command::new("pkg-config");
    pc.arg("--libs").args(group.packages);
    let pc_out = pc.output();
    if let Ok(ref output) = pc_out {
        if output.status.success() {
            let libs = String::from_utf8_lossy(&output.stdout);
            for flag in libs.split_whitespace() {
                cmd.arg(flag);
            }
            return;
        }
    }
    let reason = match &pc_out {
        Err(e) => format!("pkg-config not runnable: {e}"),
        Ok(o) => format!(
            "pkg-config exited {}: {}",
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stderr).trim()
        ),
    };
    eprintln!(
        "Warning: `pkg-config --libs {}` did not return {} linker flags \
         ({reason}). Falling back to a hardcoded {} link set — {}",
        group.packages.join(" "),
        group.what,
        group.what,
        group.advice
    );
    for lib in group.fallback_libs {
        cmd.arg(lib);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every group must query at least one package and fall back to plain
    /// `-l…` flags (a stray `-Wl,` or bare soname here would silently change
    /// the fallback link line's meaning).
    #[test]
    fn groups_are_well_formed() {
        for group in [&GTK4, &GSTREAMER, &SHUMATE, &WEBKITGTK] {
            assert!(!group.packages.is_empty(), "{}: no packages", group.what);
            assert!(
                !group.fallback_libs.is_empty(),
                "{}: no fallback libs",
                group.what
            );
            for lib in group.fallback_libs {
                assert!(
                    lib.starts_with("-l"),
                    "{}: fallback flag {lib} is not a -l flag",
                    group.what
                );
            }
            assert!(
                group.advice.contains("install"),
                "{}: advice must name the dev package to install",
                group.what
            );
        }
    }
}
