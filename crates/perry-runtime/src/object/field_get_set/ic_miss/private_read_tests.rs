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
