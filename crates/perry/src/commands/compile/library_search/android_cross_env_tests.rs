use super::android_cross_env;
use std::collections::HashMap;
use std::path::Path;

#[test]
fn x86_64_uses_matching_ndk_wrappers_and_cargo_linker() {
    let env: HashMap<_, _> = android_cross_env(Path::new("/ndk"), Some("android-x86_64"))
        .into_iter()
        .collect();
    let cc = env
        .get("CC_x86_64-linux-android")
        .expect("hyphenated cc-rs target key");
    assert!(cc.contains("x86_64-linux-android24-clang"), "{cc}");
    let linker = env
        .get("CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER")
        .expect("Cargo target linker");
    assert!(linker.contains("x86_64-linux-android24-clang"), "{linker}");
    assert!(env.contains_key("AR_x86_64_linux_android"));
}
