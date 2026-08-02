use super::apple_class_lib_name;

#[test]
fn class_stem_device_uses_canonical_name() {
    // libperry_ui_ios.a stem already carries _ios → device adds nothing.
    assert_eq!(
        apple_class_lib_name("libperry_ui_ios.a", "_ios", false),
        "libperry_ui_ios.a"
    );
}

#[test]
fn class_stem_sim_appends_sim_suffix() {
    // Same stem, simulator variant → _sim before .a.
    assert_eq!(
        apple_class_lib_name("libperry_ui_ios.a", "_ios", true),
        "libperry_ui_ios_sim.a"
    );
}

#[test]
fn generic_stem_device_appends_class() {
    // libperry_runtime.a → device gets _ios appended.
    assert_eq!(
        apple_class_lib_name("libperry_runtime.a", "_ios", false),
        "libperry_runtime_ios.a"
    );
}

#[test]
fn generic_stem_sim_appends_class_and_sim() {
    // libperry_runtime.a → simulator gets _ios_sim appended.
    assert_eq!(
        apple_class_lib_name("libperry_runtime.a", "_ios", true),
        "libperry_runtime_ios_sim.a"
    );
}

#[test]
fn handles_other_class_suffixes() {
    // Spot-check non-iOS classes to make sure the helper isn't iOS-specific.
    assert_eq!(
        apple_class_lib_name("libperry_ui_tvos.a", "_tvos", true),
        "libperry_ui_tvos_sim.a"
    );
    assert_eq!(
        apple_class_lib_name("libperry_stdlib.a", "_visionos", false),
        "libperry_stdlib_visionos.a"
    );
}
