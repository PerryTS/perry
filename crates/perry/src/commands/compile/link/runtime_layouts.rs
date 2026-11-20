use std::process::Command;

pub(super) fn add_link_args(
    cmd: &mut Command,
    darwin: bool,
    direct_apple: bool,
    xlinker_apple: bool,
) {
    if !darwin {
        return;
    }
    // Darwin needs named admission even for weak references to descriptors omitted from R.
    for name in perry_abi::NATIVE_LAYOUT_DESCRIPTORS {
        if perry_abi::NATIVE_LAYOUT_RESERVED.contains(name) {
            continue;
        }
        let symbol = format!("_perry_runtime_layout_{name}");
        if direct_apple {
            cmd.arg("-U").arg(symbol);
        } else if xlinker_apple {
            cmd.arg("-Xlinker").arg("-U").arg("-Xlinker").arg(symbol);
        } else {
            cmd.arg(format!("-Wl,-U,{symbol}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(darwin: bool, direct: bool, xlinker: bool) -> Vec<String> {
        let mut cmd = Command::new("linker");
        add_link_args(&mut cmd, darwin, direct, xlinker);
        cmd.get_args()
            .map(|arg| arg.to_str().unwrap().to_owned())
            .collect()
    }

    fn symbols() -> Vec<String> {
        perry_abi::NATIVE_LAYOUT_DESCRIPTORS
            .iter()
            .filter(|name| !perry_abi::NATIVE_LAYOUT_RESERVED.contains(name))
            .map(|name| format!("_perry_runtime_layout_{name}"))
            .collect()
    }

    #[test]
    fn driver_admits_exact_catalog_symbols_without_reserved_rows() {
        let expected: Vec<_> = symbols()
            .iter()
            .map(|name| format!("-Wl,-U,{name}"))
            .collect();
        assert!(!expected.is_empty());
        assert_eq!(arguments(true, false, false), expected);
    }

    #[test]
    fn direct_and_swift_linkers_render_named_admission() {
        let symbols = symbols();
        assert_eq!(
            arguments(true, true, false),
            symbols
                .iter()
                .flat_map(|name| ["-U".to_string(), name.clone()])
                .collect::<Vec<_>>()
        );
        assert_eq!(
            arguments(true, false, true),
            symbols
                .iter()
                .flat_map(|name| [
                    "-Xlinker".to_string(),
                    "-U".to_string(),
                    "-Xlinker".to_string(),
                    name.clone()
                ])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn non_darwin_links_receive_no_admissions() {
        for direct in [false, true] {
            for xlinker in [false, true] {
                assert!(arguments(false, direct, xlinker).is_empty());
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn omitted_weak_layout_is_null_but_required_foreign_symbols_still_fail() {
        let scratch = tempfile::tempdir().unwrap();
        let main = scratch.path().join("main.c");
        let selected = scratch.path().join("selected.c");
        std::fs::write(
            &main,
            r#"
            extern int perry_runtime_layout_READABLE_STATE __attribute__((weak_import));
            extern int perry_runtime_layout_EVENT_EMITTER __attribute__((weak_import));
            int main(void) {
                return &perry_runtime_layout_READABLE_STATE != 0 ||
                    &perry_runtime_layout_EVENT_EMITTER == 0 ||
                    perry_runtime_layout_EVENT_EMITTER != 7;
            }
        "#,
        )
        .unwrap();
        std::fs::write(&selected, "int perry_runtime_layout_EVENT_EMITTER = 7;").unwrap();
        let executable = scratch.path().join("probe");
        let baseline = Command::new("cc")
            .arg(&main)
            .arg(&selected)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(!baseline.status.success());
        assert!(String::from_utf8_lossy(&baseline.stderr)
            .contains("_perry_runtime_layout_READABLE_STATE"));
        let mut cmd = Command::new("cc");
        cmd.arg(&main).arg(&selected).arg("-o").arg(&executable);
        add_link_args(&mut cmd, true, false, false);
        let linked = cmd.output().unwrap();
        assert!(
            linked.status.success(),
            "{}",
            String::from_utf8_lossy(&linked.stderr)
        );
        assert!(Command::new(&executable).status().unwrap().success());
        std::fs::write(
            &main,
            "extern int missing_required; int main(void) { return missing_required; }",
        )
        .unwrap();
        let required = cmd.output().unwrap();
        assert!(!required.status.success());
        assert!(String::from_utf8_lossy(&required.stderr).contains("_missing_required"));
    }
}
