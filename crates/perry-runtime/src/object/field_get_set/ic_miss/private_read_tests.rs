use super::*;
#[repr(C, align(8))]
struct ImageGlobal {
    gc: crate::gc::GcHeader,
    object: ObjectHeader,
}
#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_realm")]
#[cfg_attr(target_os = "windows", link_section = ".perryrealm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_realm"
)]
static mut IMAGE_GLOBAL: ImageGlobal = ImageGlobal {
    gc: crate::gc::GcHeader {
        obj_type: crate::gc::GC_TYPE_OBJECT,
        gc_flags: crate::value::addr_class::realm::IMAGE_CELL_FLAGS,
        _reserved: crate::gc::GC_LAYOUT_POINTER_FREE,
        size: std::mem::size_of::<ImageGlobal>() as u32,
    },
    object: ObjectHeader {
        class_id: 0,
        parent_class_id: 0,
        #[cfg(target_pointer_width = "32")]
        _slot_alignment_padding: 0,
        meta: std::ptr::null_mut(),
    },
};
static PROTOTYPE_READ: IntrinsicPrivateReadSite = IntrinsicPrivateReadSite::new(
    &crate::object::runtime_layouts::intrinsic_private_atoms::REGEXP_PROTOTYPE,
);
fn image_global() -> f64 {
    let addr = unsafe { std::ptr::addr_of_mut!(IMAGE_GLOBAL.object) } as usize;
    assert!(crate::value::addr_class::realm::is_image_object(addr));
    crate::value::js_nanbox_pointer(addr as i64)
}
#[test]
fn ordinary_private_slot_miss_is_not_an_intrinsic_value_read() {
    crate::test_support::bootstrap_test_thread();
    let site = OwnPrivateReadSite::new(
        &crate::object::runtime_layouts::intrinsic_private_atoms::REGEXP_PROTOTYPE,
    );
    assert!(site.read(image_global()).is_none());
}
#[test]
fn runtime_value_read_of_unrooted_cell_aborts_by_name() {
    const CHILD: &str = "PERRY_REALM_PRIVATE_READ_DEATH";
    if std::env::var_os(CHILD).is_some() {
        crate::test_support::bootstrap_test_thread();
        let _ = crate::realm_value_read! { "RegExp" => "RegExp.prototype"; PROTOTYPE_READ.read(image_global()) };
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "runtime_value_read_of_unrooted_cell_aborts_by_name",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(!output.status.success(), "unrooted read succeeded");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("runtime value read of RegExp.prototype is unrooted"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn projected_runtime_value_root_is_readable_without_a_birth_fallback() {
    const CHILD: &str = "REALM_ROOTS_PROJECTION_CHILD";
    if std::env::var_os(CHILD).is_some() {
        crate::test_support::bootstrap_test_thread();
        let held = std::env::var_os("REALM_ROOTS_PROJECTION_PATH")
            .map(|path| {
                std::fs::read_to_string(path)
                    .unwrap()
                    .lines()
                    .any(|name| name == "Ctor.prototype")
            })
            .unwrap_or(true);
        // The fixture's held pointer is an image cell, never a collectable
        // edge. A missing close referent leaves the private slot undefined,
        // exactly as the image value projection does.
        let global = image_global();
        let value = if held {
            global
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        crate::object::intrinsic_private_add(global, "[[RegExpPrototype]]", value);
        let value = crate::realm_value_read! { "RegExp" => "RegExp.prototype"; PROTOTYPE_READ.read(global) };
        assert_eq!(value.unwrap().to_bits(), global.to_bits());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "object::field_get_set::ic_miss::private_read_tests::projected_runtime_value_root_is_readable_without_a_birth_fallback", "--nocapture"])
        .env(CHILD, "1").output().unwrap();
    assert!(
        output.status.success(),
        "closed runtime value root was not readable: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
