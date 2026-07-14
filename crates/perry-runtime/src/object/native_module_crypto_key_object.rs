use crate::JSValue;

fn value_addr(value: f64) -> usize {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    }
}

fn invalid_key(value: f64) -> ! {
    let message = format!(
        "The \"key\" argument must be an instance of CryptoKey. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

/// A CryptoKey Perry recognises but cannot turn into a KeyObject must still
/// fail loudly. Returning `undefined` from a factory like this is what #6302
/// was filed about.
fn conversion_failed(detail: &str) -> ! {
    let message = format!("KeyObject.from() failed: {detail}");
    crate::fs::validate::throw_error_with_code(&message, "ERR_CRYPTO_OPERATION_FAILED")
}

/// `crypto.KeyObject.from(cryptoKey)` (#2565; asymmetric + unregistered-buffer
/// keys #6302).
///
/// Node converts *any* CryptoKey — secret, public, or private — into the
/// matching KeyObject, and throws `ERR_INVALID_ARG_TYPE` for everything else.
/// Perry models the two KeyObject shapes differently:
///
/// * **secret** — the key bytes in a `BufferHeader` flagged by
///   `mark_as_secret_key`, which is what the KeyObject property/method surface
///   keys off (`type`, `symmetricKeySize`, `export`).
/// * **asymmetric** — a PEM (RSA/EC) or internal Ed/X surrogate *string*
///   flagged by `mark_as_asymmetric_key`.
///
/// The secret conversion is a buffer copy and happens here; the asymmetric one
/// needs the key encoders (RSA PKCS#8/SPKI, P-256/384/521, Ed25519, X25519)
/// that live in perry-stdlib, so it is routed through the WebCrypto dispatch
/// hook — the same bridge `keyObject.toCryptoKey()` uses in the other
/// direction (`native_call_method/string_methods.rs`).
///
/// Pre-#6302 this function demanded `kind == 1 && is_registered_buffer(addr)`
/// and sent everything else to `invalid_key`, so a public/private CryptoKey —
/// or a secret CryptoKey whose backing buffer was not in *this* thread's buffer
/// registry — never produced a KeyObject at all.
pub(super) unsafe fn key_object_from(value: f64) -> f64 {
    let addr = value_addr(value);
    let Some((_algo, _hash, kind, _extractable, _usages)) = crate::buffer::crypto_key_meta(addr)
    else {
        invalid_key(value);
    };
    match kind {
        1 => secret_key_object(addr),
        2 | 3 => asymmetric_key_object(addr),
        _ => invalid_key(value),
    }
}

/// Copy the CryptoKey's bytes into a fresh secret-key Buffer.
///
/// The old `is_registered_buffer(addr)` precondition was wrong: CryptoKey
/// metadata is only ever attached to a `BufferHeader` allocated by the
/// WebCrypto key factories, and that metadata is *also* kept in a
/// process-global registry, so the address is readable even when the current
/// thread's (thread-local) buffer registry has no entry for it. Requiring the
/// registry hit turned such keys into a silent `invalid_key` throw.
unsafe fn secret_key_object(addr: usize) -> f64 {
    let src = addr as *const crate::buffer::BufferHeader;
    let len = (*src).length as usize;
    let out = crate::buffer::buffer_alloc(len as u32);
    if out.is_null() {
        conversion_failed("could not allocate the secret key buffer");
    }
    if len > 0 {
        std::ptr::copy_nonoverlapping(
            crate::buffer::buffer_data(src),
            crate::buffer::buffer_data_mut(out),
            len,
        );
    }
    (*out).length = len as u32;
    crate::buffer::mark_as_uint8array(out as usize);
    crate::buffer::mark_as_secret_key(out as usize);
    f64::from_bits(JSValue::pointer(out as *const u8).bits())
}

/// Public/private CryptoKey → PEM/surrogate-string KeyObject, encoded by
/// perry-stdlib's WebCrypto bridge (it owns the RSA/EC/Ed/X key writers and
/// the CryptoKey material registry).
unsafe fn asymmetric_key_object(addr: usize) -> f64 {
    let ptr = crate::value::JS_NATIVE_WEBCRYPTO_DISPATCH.load(std::sync::atomic::Ordering::SeqCst);
    if ptr.is_null() {
        conversion_failed("the WebCrypto runtime is not available");
    }
    let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
        std::mem::transmute(ptr);
    // Hand the bridge a properly NaN-boxed pointer: a Buffer reaching
    // `KeyObject.from` can arrive as a raw f64-bitcast pointer (module-level
    // storage convention), which the stdlib pointer-strip helper would read as
    // a tagged primitive and reject.
    let args = [f64::from_bits(JSValue::pointer(addr as *const u8).bits())];
    let method = "keyObjectFromCryptoKey";
    let result = dispatch(method.as_ptr(), method.len(), args.as_ptr(), args.len());
    // The bridge throws for key material it cannot model; a bare `undefined`
    // return would be the exact silent failure #6302 is about.
    if JSValue::from_bits(result.to_bits()).is_undefined() {
        conversion_failed("unsupported asymmetric key type");
    }
    result
}
