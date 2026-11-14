//! Declarations live only in the build projection. Release reads stay unchanged.
#[macro_export]
macro_rules! realm_value_read {
    ($subject:literal => $target:literal; $expression:expr) => {
        $expression
    };
    (table = GLOBAL_THIS_BUILTIN_CONSTRUCTORS; $expression:expr) => {
        $expression
    };
}

#[cfg(any(test, debug_assertions, perry_diagnostics))]
#[cold]
#[inline(never)]
fn undefined_realm_value(target: &str) -> ! {
    eprintln!(
        "Perry internal error: runtime value read of {target} is unrooted in the realm image"
    );
    std::process::abort();
}

#[cfg(any(test, debug_assertions, perry_diagnostics))]
pub(crate) fn check_image_intrinsic_read(addr: usize, name: &str, prototype: bool) {
    if addr == 0 && crate::object::global_this::has_image_global() {
        let target = if prototype {
            format!("{name}.prototype")
        } else {
            name.to_owned()
        };
        undefined_realm_value(&target);
    }
}

#[cfg(any(test, debug_assertions, perry_diagnostics))]
pub(crate) fn check_private_read(receiver: f64, value: Option<f64>, spelling: &[u8]) {
    let addr = (receiver.to_bits() & crate::value::POINTER_MASK) as usize;
    if crate::value::addr_class::realm::is_image_object(addr)
        && value.is_none_or(|v| v.to_bits() == crate::value::TAG_UNDEFINED || v == 0.0)
    {
        // Qualified intrinsic private keys carry their own name. They are
        // atoms minted from UTF-8 source, not a parallel name registry.
        let spelling = std::str::from_utf8(spelling).expect("intrinsic private key is UTF-8");
        let (kind, part) = perry_abi::realm_private::intrinsic_target(spelling)
            .expect("intrinsic private key has its target role");
        let target = match part {
            perry_abi::realm_private::IntrinsicPart::Prototype => format!("{kind}.prototype"),
            perry_abi::realm_private::IntrinsicPart::Constructor => kind.to_owned(),
        };
        undefined_realm_value(&target);
    }
}

/// A complete named lookup proved absence on an unchanged STRUCTURE cell.
/// Such a read was not covered by the closed reach facts. Kept undefined
/// values and getter results never call this miss entrance.
#[cfg(any(test, debug_assertions, perry_diagnostics))]
#[cold]
#[inline(never)]
pub(crate) fn check_structure_named_miss(receiver: usize, key_bits: u64) {
    if key_bits & crate::value::TAG_MASK != crate::value::STRING_TAG {
        return;
    }
    let key = (key_bits & crate::value::POINTER_MASK) as *const crate::StringHeader;
    let name = unsafe {
        std::slice::from_raw_parts(crate::string::string_data(key), (*key).byte_len as usize)
    };
    check_structure_named_text_miss(receiver, name);
}

#[cfg(any(test, debug_assertions, perry_diagnostics))]
#[cold]
#[inline(never)]
pub(crate) fn check_structure_named_text_miss(receiver: usize, name: &[u8]) {
    if !unsafe { crate::object::layout_desc::structure_receiver(receiver) } {
        return;
    }
    eprintln!(
        "Perry internal error: named read {} misses a STRUCTURE realm image cell",
        String::from_utf8_lossy(name)
    );
    std::process::abort();
}

/// Generic lookup helpers can conflate absence with a present undefined
/// value. Their diagnostic uses the existing value-free presence walk,
/// so it never invokes an inherited getter again to decide the miss.
#[cfg(any(test, debug_assertions, perry_diagnostics))]
#[cold]
#[inline(never)]
pub(crate) fn check_structure_named_lookup_miss(receiver: usize, key_bits: u64) {
    if unsafe { crate::object::layout_desc::structure_receiver(receiver) }
        && unsafe {
            crate::object::layout_probe::probe_has(
                crate::value::js_nanbox_pointer(receiver as i64).to_bits(),
                key_bits,
                false,
            )
        } == 0
    {
        check_structure_named_miss(receiver, key_bits);
    }
}
