//! Embed the checked-in Gradle project without development-machine build output.
use std::{env, fs, io, path::Path};

pub fn emit() -> io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../perry-ui-android/template");
    let mut files = Vec::new();
    collect(&root, &root, &mut files)?;
    files.sort();
    let mut source = String::from("pub(super) static FILES: &[(&str, &[u8])] = &[\n");
    for name in files {
        let path = root.join(&name);
        println!("cargo:rerun-if-changed={}", path.display());
        source.push_str(&format!(
            "    ({:?}, include_bytes!({:?})),\n",
            name.replace('\\', "/"),
            path.to_str().expect("Android template path must be UTF-8")
        ));
    }
    source.push_str("];");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("android_template.rs"),
        source,
    )
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<String>) -> io::Result<()> {
    // Watching directories also detects newly added template resources.
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let relative = entry
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/");
        // Gradle caches, generated wrappers/builds, machine-specific SDK paths,
        // and native outputs are not template sources. They must not be shipped.
        if name.starts_with('.')
            || matches!(
                name.as_ref(),
                "build" | "gradlew" | "gradlew.bat" | "local.properties" | "jniLibs"
            )
            || relative == "gradle/wrapper"
        {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Android template must not contain symlinks: {}",
                    entry.path().display()
                ),
            ));
        }
        if kind.is_dir() {
            collect(root, &entry.path(), files)?;
        } else if kind.is_file() {
            files.push(relative);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_template_collector_includes_new_resources_but_not_machine_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for name in [
            "gradle/libs.versions.toml",
            "app/src/main/res/drawable/new_icon.png",
            "app/src/main/java/com/perry/app/NewBridge.kt",
            "local.properties",
            ".gradle/cache",
            "app/build/generated/secret",
            "app/src/main/jniLibs/arm64-v8a/libperry_app.so",
            "gradlew",
            "gradle/wrapper/gradle-wrapper.jar",
        ] {
            let file = root.join(name);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, [0, 255, 128, 10]).unwrap();
        }
        let mut files = Vec::new();
        collect(root, root, &mut files).unwrap();
        files.sort();
        assert_eq!(
            files,
            [
                "app/src/main/java/com/perry/app/NewBridge.kt",
                "app/src/main/res/drawable/new_icon.png",
                "gradle/libs.versions.toml",
            ]
        );
    }
}
