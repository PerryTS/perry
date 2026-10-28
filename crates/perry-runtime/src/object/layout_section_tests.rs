//! Exercise the linked image, including COFF's separate read-only/writable
//! atom sections. Looking only at the declarations cannot catch empty bounds.
use super::*;

#[cfg(not(target_family = "wasm"))]
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static mut WRITABLE_ATOM: StaticAtom = StaticAtom {
    hash: 0x12345678,
    string: std::ptr::null(),
    kind: crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL,
    _pad: [0; 3],
    index: 0,
};

#[test]
#[cfg(not(target_family = "wasm"))]
fn linked_layout_section_contains_runtime_prototypes() {
    let descriptor = std::ptr::addr_of!(crate::object::runtime_layouts::OBJECT_PROTOTYPE);
    let matches = image_layouts()
        .filter(|p| p.cast_const() == descriptor)
        .count();
    assert_eq!(
        matches, 1,
        "runtime prototype must be inside the scanned section exactly once"
    );
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn linked_atom_sections_include_readonly_and_writable_records() {
    let descriptor = std::ptr::addr_of!(crate::object::runtime_layouts::OBJECT_PROTOTYPE);
    unsafe extern "C" {
        #[link_name = "perry_atom_636f6e7374727563746f72"]
        static CONSTRUCTOR_ATOM: StaticAtom;
    }
    // The descriptor may use a const-readable view of the atom's facts.
    // Section enumeration owns the canonical record, not that private view.
    let readonly = std::ptr::addr_of!(CONSTRUCTOR_ATOM);
    assert_eq!(
        unsafe { (*(*(*descriptor).keys).atom).string },
        unsafe { (*readonly).string },
        "descriptor facts must name the canonical image string"
    );
    let writable = std::ptr::addr_of!(WRITABLE_ATOM);
    let atoms: Vec<_> = image_atoms().map(|p| p.cast_const()).collect();
    assert_eq!(atoms.iter().filter(|&&p| p == readonly).count(), 1);
    assert_eq!(atoms.iter().filter(|&&p| p == writable).count(), 1);
}
