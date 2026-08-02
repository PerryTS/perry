use super::versioned_llvm_bin_dirs;
use std::path::PathBuf;

// #5779: the well-known-LLVM-dir fallback must return versioned kegs
// newest-first, so bitcode inspection uses an LLVM at least as new as the
// Rust toolchain's. Ordering is the only non-trivial part, and it is pure,
// so exercise it directly without touching the filesystem.
#[test]
fn versioned_llvm_dirs_are_newest_first() {
    let names = vec![
        "llvm@15".to_string(),
        "llvm@18".to_string(),
        "llvm@9".to_string(),
        "llvm".to_string(),    // unversioned — ignored here
        "node@20".to_string(), // wrong prefix — ignored
    ];
    let dirs = versioned_llvm_bin_dirs("/opt/homebrew/opt", "llvm@", &names);
    assert_eq!(
        dirs,
        vec![
            PathBuf::from("/opt/homebrew/opt/llvm@18/bin"),
            PathBuf::from("/opt/homebrew/opt/llvm@15/bin"),
            PathBuf::from("/opt/homebrew/opt/llvm@9/bin"),
        ]
    );
}

#[test]
fn versioned_llvm_dirs_handles_debian_prefix_and_ignores_nonmatching() {
    let names = vec![
        "llvm-17".to_string(),
        "llvm-16.0".to_string(),
        "gcc-12".to_string(),
        "llvm-".to_string(), // no numeric version — ignored
    ];
    let dirs = versioned_llvm_bin_dirs("/usr/lib", "llvm-", &names);
    assert_eq!(
        dirs,
        vec![
            PathBuf::from("/usr/lib/llvm-17/bin"),
            PathBuf::from("/usr/lib/llvm-16.0/bin"),
        ]
    );
}
