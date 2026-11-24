//! Codegen-unit objects that must outlive their merge (`--debug-symbols`, macOS).
//!
//! `ld -r` on Mach-O copies no DWARF into the merged object. It leaves a debug
//! map naming the unit objects, and `dsymutil` reads them after the final link.

use std::path::PathBuf;
use std::sync::Mutex;

static RETAINED: Mutex<Option<Vec<PathBuf>>> = Mutex::new(None);

/// While alive, merged unit objects stay on disk; dropping it removes them.
pub struct RetainUnitObjects(());

impl RetainUnitObjects {
    pub fn begin() -> Self {
        RETAINED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_default();
        Self(())
    }
}

impl Drop for RetainUnitObjects {
    fn drop(&mut self) {
        let retained = RETAINED.lock().unwrap_or_else(|e| e.into_inner()).take();
        for path in retained.unwrap_or_default() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Take ownership of `paths`, or return `false` when nothing retains them.
#[cfg(not(target_os = "windows"))]
pub(crate) fn retain(paths: &[PathBuf]) -> bool {
    match RETAINED.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        Some(retained) => {
            retained.extend_from_slice(paths);
            true
        }
        None => false,
    }
}

pub(crate) fn unit_object_name(nonce: u64, index: usize) -> String {
    format!("{}{nonce}_{index}.o", name_prefix())
}

fn name_prefix() -> String {
    format!("perry_cgu_{}_", std::process::id())
}

/// Whether `object`'s debug map names a retained unit object. Such an object
/// must not enter a cache: a later build would find the map's targets gone.
pub fn names_retained_unit(object: &[u8]) -> bool {
    if RETAINED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_none_or(Vec::is_empty)
    {
        return false;
    }
    let prefix = name_prefix();
    object
        .windows(prefix.len())
        .any(|window| window == prefix.as_bytes())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn unit(name: &str) -> Vec<u8> {
        let ll = format!(
            "define double @{name}() !dbg !3 {{\n  ret double 0.0, !dbg !6\n}}\n\
             !llvm.dbg.cu = !{{!0}}\n!llvm.module.flags = !{{!2}}\n\
             !0 = distinct !DICompileUnit(language: DW_LANG_C99, file: !1, emissionKind: FullDebug)\n\
             !1 = !DIFile(filename: \"{name}.ts\", directory: \"/src\")\n\
             !2 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n\
             !3 = distinct !DISubprogram(name: \"{name}\", file: !1, line: 1, type: !4, unit: !0, spFlags: DISPFlagDefinition)\n\
             !4 = !DISubroutineType(types: !5)\n!5 = !{{}}\n!6 = !DILocation(line: 1, scope: !3)\n"
        );
        crate::linker::compile_ll_to_object(&ll, None).expect("unit compiles")
    }

    #[test]
    fn unit_objects_live_until_the_guard_drops() {
        let units = [unit("cgu_a"), unit("cgu_b")];

        let unretained = crate::linker::merge_unit_objects(&units).expect("merge");
        assert!(!names_retained_unit(&unretained));

        let guard = RetainUnitObjects::begin();
        let merged = crate::linker::merge_unit_objects(&units).expect("merge");
        assert!(
            names_retained_unit(&merged),
            "the debug map names the unit objects"
        );
        assert!(
            !names_retained_unit(&units[0]),
            "an unmerged object carries its own DWARF"
        );
        let paths = RETAINED.lock().unwrap().clone().expect("guard is alive");
        assert!(paths.len() >= 2 && paths.iter().all(|path| path.is_file()));

        drop(guard);
        assert!(paths.iter().all(|path| !path.exists()));
    }
}
