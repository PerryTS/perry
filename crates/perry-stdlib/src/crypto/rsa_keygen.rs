//! RSA key generation, shared by `node:crypto` and WebCrypto.

use super::*;
use perry_runtime::fs::validate::{
    describe_received, format_received_number, is_numeric, throw_range_error_with_code,
    throw_type_error_with_code,
};

const MIN_MODULUS_BITS: u32 = 512;
/// Past this size a prime no longer fits an OpenSSL BIGNUM.
const MAX_MODULUS_BITS: u32 = 1 << 30;
const DEFAULT_PUBLIC_EXPONENT: u32 = 65537;

/// Why no key exists for a size and exponent, worded as Node words it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RsaKeygenError {
    message: &'static str,
    code: Option<&'static str>,
}

const KEY_SIZE_TOO_SMALL: RsaKeygenError = RsaKeygenError {
    message: "error:1C8000AB:Provider routines::key size too small",
    code: Some("ERR_OSSL_KEY_SIZE_TOO_SMALL"),
};
/// The size as OpenSSL's signed int is negative.
const KEY_SIZE_OVERFLOW: RsaKeygenError = RsaKeygenError {
    message: "error:02000078:rsa routines::key size too small",
    code: Some("ERR_OSSL_RSA_KEY_SIZE_TOO_SMALL"),
};
const KEY_SIZE_TOO_LARGE: RsaKeygenError = RsaKeygenError {
    message: "error:01800072:bignum routines::bignum too long",
    code: Some("ERR_OSSL_BN_BIGNUM_TOO_LONG"),
};
const PUBLIC_EXPONENT: RsaKeygenError = RsaKeygenError {
    message: "error:020000B2:rsa routines::pub exponent out of range",
    code: Some("ERR_OSSL_RSA_PUB_EXPONENT_OUT_OF_RANGE"),
};
const JOB_FAILED: RsaKeygenError = RsaKeygenError {
    message: "Key generation job failed",
    code: None,
};

impl RsaKeygenError {
    pub(super) fn into_error(self) -> perry_runtime::error::CoreError {
        let message = js_string_from_bytes(self.message.as_ptr(), self.message.len() as u32);
        if let Some(code) = self.code {
            perry_runtime::node_submodules::register_error_code_pub(message, code);
        }
        perry_runtime::error::CoreError::error(message)
    }

    pub(super) fn throw(self) -> ! {
        perry_runtime::exception::throw_core(self.into_error())
    }
}

pub(crate) fn generate_rsa_key(
    modulus_bits: u32,
    public_exponent: u32,
) -> Result<RsaPrivateKey, RsaKeygenError> {
    let bits = checked_modulus_bits(modulus_bits, public_exponent)?;
    RsaPrivateKey::new_with_exp(
        &mut rand_core_06::OsRng,
        bits as usize,
        &RsaBigUint::from(public_exponent),
    )
    .map_err(|_| JOB_FAILED)
}

/// The modulus size OpenSSL generates for a request. The checks keep its
/// order, so a request with several faults reports the one Node reports.
fn checked_modulus_bits(modulus_bits: u32, public_exponent: u32) -> Result<u32, RsaKeygenError> {
    if modulus_bits < MIN_MODULUS_BITS {
        return Err(KEY_SIZE_TOO_SMALL);
    }
    if public_exponent == 0 {
        return Err(JOB_FAILED);
    }
    if modulus_bits > i32::MAX as u32 {
        return Err(KEY_SIZE_OVERFLOW);
    }
    if public_exponent < 3 || public_exponent % 2 == 0 {
        return Err(PUBLIC_EXPONENT);
    }
    if modulus_bits > MAX_MODULUS_BITS {
        return Err(KEY_SIZE_TOO_LARGE);
    }
    // OpenSSL takes SP 800-56B's two equal primes from 2048 bits on when the
    // exponent exceeds 16 bits, so an odd size loses its last bit.
    if modulus_bits >= 2048 && public_exponent > 0xFFFF {
        return Ok(modulus_bits & !1);
    }
    Ok(modulus_bits)
}

/// `modulusLength` and `publicExponent` of `generateKeyPair`'s options.
pub(super) unsafe fn rsa_keygen_options(options: f64) -> (u32, u32) {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    require_options_object(options.get_nanbox_f64());
    let read = |name: &str| {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let object = JSValue::from_bits(options.get_nanbox_f64().to_bits()).as_pointer();
        js_object_get_field_by_name(object, key)
    };
    let modulus_bits = require_u32("modulusLength", read("modulusLength"));
    let public_exponent = match read("publicExponent") {
        value if value.is_undefined() || value.is_null() => DEFAULT_PUBLIC_EXPONENT,
        value => require_u32("publicExponent", value),
    };
    (modulus_bits, public_exponent)
}

unsafe fn require_options_object(options: f64) {
    let value = JSValue::from_bits(options.to_bits());
    let is_array = JSValue::from_bits(perry_runtime::js_array_is_array(options).to_bits());
    let is_object = value.is_pointer()
        && perry_runtime::symbol::js_is_symbol(options) == 0
        && !(is_array.is_bool() && is_array.as_bool())
        && !perry_runtime::closure::is_closure_ptr(value.as_pointer::<u8>() as usize);
    if !is_object {
        let message = format!(
            "The \"options\" argument must be of type object. Received {}",
            describe_received(options)
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
}

fn require_u32(name: &str, value: JSValue) -> u32 {
    if !is_numeric(value) {
        let message = format!(
            "The \"options.{name}\" property must be of type number. Received {}",
            describe_received(f64::from_bits(value.bits()))
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let number = value.as_number();
    let expected = if !number.is_finite() || number.fract() != 0.0 {
        "an integer".to_string()
    } else if !(0.0..=f64::from(u32::MAX)).contains(&number) {
        format!(">= 0 && <= {}", u32::MAX)
    } else {
        return number as u32;
    };
    throw_range_error_with_code(&format!(
        "The value of \"options.{name}\" is out of range. It must be {expected}. Received {}",
        format_received_number(number)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::traits::PublicKeyParts;

    #[test]
    fn generates_the_requested_size_and_exponent() {
        for (bits, exponent) in [
            (512, 3),
            (1023, 65537),
            (1024, 17),
            (1024, u32::MAX),
            (3072, 65537),
        ] {
            let key = generate_rsa_key(bits, exponent).unwrap();
            assert_eq!(key.n().bits(), bits as usize);
            assert_eq!(key.e(), &RsaBigUint::from(exponent));
            key.validate().unwrap();
        }
    }

    #[test]
    fn a_public_key_loads_up_to_openssl_limit() {
        let modulus = |bits: usize| (RsaBigUint::from(1u8) << (bits - 1)) + 1u8;
        let load = |bits| rsa_public_key(modulus(bits), RsaBigUint::from(65537u32));
        assert!(load(4097).is_some());
        assert!(load(16384).is_some());
        assert!(load(16385).is_none());
    }

    #[test]
    fn an_odd_size_follows_openssl() {
        assert_eq!(checked_modulus_bits(2047, 65537), Ok(2047));
        assert_eq!(checked_modulus_bits(2049, 65537), Ok(2048));
        assert_eq!(checked_modulus_bits(3073, u32::MAX), Ok(3072));
        assert_eq!(checked_modulus_bits(2049, 65535), Ok(2049));
        assert_eq!(checked_modulus_bits(4096, 65537), Ok(4096));
    }

    #[test]
    fn rejects_what_openssl_rejects() {
        for (bits, exponent, error) in [
            (0, 65537, KEY_SIZE_TOO_SMALL),
            (511, 2, KEY_SIZE_TOO_SMALL),
            (8, 0, KEY_SIZE_TOO_SMALL),
            (2048, 0, JOB_FAILED),
            (2048, 1, PUBLIC_EXPONENT),
            (2048, 2, PUBLIC_EXPONENT),
            (2048, 65536, PUBLIC_EXPONENT),
            ((1 << 30) + 1, 4, PUBLIC_EXPONENT),
            ((1 << 30) + 1, 65537, KEY_SIZE_TOO_LARGE),
            (1 << 31, 0, JOB_FAILED),
            (1 << 31, 4, KEY_SIZE_OVERFLOW),
            (u32::MAX, 65537, KEY_SIZE_OVERFLOW),
        ] {
            assert_eq!(
                generate_rsa_key(bits, exponent).err(),
                Some(error),
                "{bits} {exponent}"
            );
        }
    }
}
