//! Compile-time classification of population rows. Runtime selection reads
//! these flags rather than repeating the ABI's name match for each pass.

const fn same_name(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

pub(crate) const fn contains(names: &[&str], name: &str) -> bool {
    let mut index = 0;
    while index < names.len() {
        if same_name(names[index], name) {
            return true;
        }
        index += 1;
    }
    false
}

pub(crate) const fn in_rest(name: &str) -> bool {
    contains(crate::codegen_abi::REALM_REST_GLOBALS, name)
}

pub(crate) const fn rest_mask(names: &[&str]) -> u64 {
    assert!(names.len() <= u64::BITS as usize);
    let mut mask = 0;
    let mut index = 0;
    while index < names.len() {
        if in_rest(names[index]) {
            mask |= 1 << index;
        }
        index += 1;
    }
    mask
}

pub(crate) const fn realm_seals_prototype(name: &str) -> bool {
    let names = crate::codegen_abi::BuiltinProto::REALM;
    let mut index = 0;
    while index < names.len() {
        if same_name(names[index].1, name) {
            return true;
        }
        index += 1;
    }
    false
}
