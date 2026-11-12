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
        let role = spelling
            .rsplit_once("[[")
            .expect("qualified intrinsic key")
            .1
            .strip_suffix("]]>")
            .expect("qualified intrinsic key suffix");
        let target = if let Some(kind) = role.strip_suffix("Prototype") {
            format!("{kind}.prototype")
        } else if let Some(kind) = role.strip_suffix("Constructor") {
            kind.to_owned()
        } else {
            role.to_owned()
        };
        undefined_realm_value(&target);
    }
}
