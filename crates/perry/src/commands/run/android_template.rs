//! The Android/Wear OS project ships inside the compiler, including resources
//! and Kotlin/Java bridge sources. build.rs derives this table from the template.
use std::{fs, io, path::Path};

include!(concat!(env!("OUT_DIR"), "/android_template.rs"));

pub(super) fn extract(destination: &Path) -> io::Result<()> {
    for (relative, contents) in FILES {
        let path = destination.join(relative);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, contents)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_android_template_extracts_without_a_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("release-app/android-build");
        extract(&destination).unwrap();
        // Required packaging/bridge inputs, not just a non-empty output tree.
        for relative in [
            "build.gradle.kts",
            "settings.gradle.kts",
            "gradle.properties",
            "app/build.gradle.kts",
            "app/src/main/AndroidManifest.xml",
            "app/src/main/java/com/perry/app/PerryActivity.kt",
            "app/src/main/java/com/perry/app/PerryBridge.kt",
            "app/src/main/java/com/perry/app/PerryMediaSessionCallback.java",
            "app/src/main/res/values/themes.xml",
            "app/src/main/res/drawable/splash_background.xml",
        ] {
            assert!(destination.join(relative).is_file(), "missing {relative}");
        }
        for (relative, bytes) in FILES {
            assert_eq!(
                fs::read(destination.join(relative)).unwrap(),
                *bytes,
                "{relative}"
            );
        }
        assert!(!destination.join("local.properties").exists());
        assert!(!destination.join("app/src/main/jniLibs").exists());
        let manifest =
            fs::read_to_string(destination.join("app/src/main/AndroidManifest.xml")).unwrap();
        assert!(manifest.contains("PerryActivity"));
        let gradle = fs::read_to_string(destination.join("app/build.gradle.kts")).unwrap();
        assert!(gradle.contains("applicationId = \"com.perry.template\""));
    }

    #[test]
    fn bundled_android_template_matches_checked_in_sources() {
        // Every embedded file must match the source tree byte-for-byte. This
        // catches lossy string conversions and platform newline rewriting.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../perry-ui-android/template");
        for (relative, contents) in FILES {
            assert_eq!(
                fs::read(root.join(relative)).unwrap(),
                *contents,
                "{relative}"
            );
        }
    }
}

// Run the build-time collector's regression cases in the CLI test harness.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../build/android_template.rs"]
mod embedding;
