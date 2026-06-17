use super::*;
use ml_kem::kem::Decapsulate;
use rand::RngCore;

unsafe fn reject_type_error_with_code(message: &str, code: &'static str) -> *mut Promise {
    let msg = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(msg, code);
    let err = perry_runtime::error::js_typeerror_new(msg);
    let value = f64::from_bits(JSValue::pointer(err as *const u8).bits());
    perry_runtime::js_promise_rejected(value)
}

unsafe fn reject_missing_args(method: &str, required: usize, present: usize) -> *mut Promise {
    let message = format!(
        "Failed to execute '{method}' on 'SubtleCrypto': {required} arguments required, but only {present} present."
    );
    reject_type_error_with_code(&message, "ERR_MISSING_ARGS")
}

unsafe fn reject_unsupported_algorithm() -> *mut Promise {
    reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
}

unsafe fn check_required_args(
    method: &str,
    required: usize,
    args_len: usize,
) -> Option<*mut Promise> {
    if args_len < required {
        return Some(reject_missing_args(method, required, args_len));
    }
    None
}

unsafe fn nth_arg(args_ptr: *const f64, args_len: usize, index: usize) -> f64 {
    if args_ptr.is_null() || index >= args_len {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        *args_ptr.add(index)
    }
}

fn ml_kem_encapsulate(algo: KeyAlgo, public_der: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let public_bytes = ml_kem_public_bytes_from_der(algo, public_der)?;
    let mut random = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut random);
    let m = ml_kem::B32::try_from(random.as_slice()).ok()?;
    match algo {
        KeyAlgo::MlKem512 => {
            let public =
                ml_kem::Key::<ml_kem::EncapsulationKey512>::try_from(public_bytes.as_slice())
                    .ok()?;
            let key = ml_kem::EncapsulationKey512::new(&public).ok()?;
            let (ciphertext, shared) = key.encapsulate_deterministic(&m);
            Some((ciphertext.as_slice().to_vec(), shared.as_slice().to_vec()))
        }
        KeyAlgo::MlKem768 => {
            let public =
                ml_kem::Key::<ml_kem::EncapsulationKey768>::try_from(public_bytes.as_slice())
                    .ok()?;
            let key = ml_kem::EncapsulationKey768::new(&public).ok()?;
            let (ciphertext, shared) = key.encapsulate_deterministic(&m);
            Some((ciphertext.as_slice().to_vec(), shared.as_slice().to_vec()))
        }
        KeyAlgo::MlKem1024 => {
            let public =
                ml_kem::Key::<ml_kem::EncapsulationKey1024>::try_from(public_bytes.as_slice())
                    .ok()?;
            let key = ml_kem::EncapsulationKey1024::new(&public).ok()?;
            let (ciphertext, shared) = key.encapsulate_deterministic(&m);
            Some((ciphertext.as_slice().to_vec(), shared.as_slice().to_vec()))
        }
        _ => None,
    }
}

fn ml_kem_decapsulate(algo: KeyAlgo, private_der: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    let (seed_bytes, _) = ml_kem_private_seed_and_public_from_der(algo, private_der)?;
    let seed = ml_kem::Seed::try_from(seed_bytes.as_slice()).ok()?;
    match algo {
        KeyAlgo::MlKem512 => {
            let private = ml_kem::DecapsulationKey512::from_seed(seed);
            let ct: ml_kem::ml_kem_512::Ciphertext = ciphertext.try_into().ok()?;
            Some(private.decapsulate(&ct).as_slice().to_vec())
        }
        KeyAlgo::MlKem768 => {
            let private = ml_kem::DecapsulationKey768::from_seed(seed);
            let ct: ml_kem::ml_kem_768::Ciphertext = ciphertext.try_into().ok()?;
            Some(private.decapsulate(&ct).as_slice().to_vec())
        }
        KeyAlgo::MlKem1024 => {
            let private = ml_kem::DecapsulationKey1024::from_seed(seed);
            let ct: ml_kem::ml_kem_1024::Ciphertext = ciphertext.try_into().ok()?;
            Some(private.decapsulate(&ct).as_slice().to_vec())
        }
        _ => None,
    }
}

unsafe fn ml_kem_algo_arg(algo_bits: u64) -> Result<KeyAlgo, *mut Promise> {
    let Some(name) = extract_algo_name(algo_bits) else {
        return Err(reject_unsupported_algorithm());
    };
    ml_kem_key_algo_from_name(&name.to_ascii_uppercase())
        .ok_or_else(|| reject_unsupported_algorithm())
}

unsafe fn key_material_arg(
    key_bits: u64,
    algo: KeyAlgo,
    expected_kind: KeyKind,
    usage: u32,
    usage_message: &'static str,
    invalid_type_message: &'static str,
) -> Result<Vec<u8>, *mut Promise> {
    let key_addr = strip_ptr(key_bits);
    let Some(mat) = lookup_crypto_key(key_addr) else {
        return Err(reject_type_error_with_code(
            invalid_type_message,
            "ERR_INVALID_ARG_TYPE",
        ));
    };
    if mat.algo != algo {
        return Err(reject_with_dom_exception(
            "InvalidAccessError",
            "key algorithm mismatch",
        ));
    }
    if mat.kind != expected_kind {
        return Err(reject_with_dom_exception(
            "InvalidAccessError",
            "The requested operation is not valid for the provided key",
        ));
    }
    if let Err((name, message)) = require_usage(mat, usage, usage_message) {
        return Err(reject_with_dom_exception(name, message));
    }
    Ok(bytes_from_jsvalue(key_bits))
}

unsafe fn object_with_two_buffers(
    first_name: &[u8],
    first_bytes: &[u8],
    second_name: &[u8],
    second_bytes: &[u8],
) -> *mut Promise {
    let first = alloc_uint8array_from_slice(first_bytes);
    let second = alloc_uint8array_from_slice(second_bytes);
    if first.is_null() || second.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let obj = js_object_alloc(0, 2);
    if obj.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let first_key =
        perry_runtime::js_string_from_bytes(first_name.as_ptr(), first_name.len() as u32);
    let second_key =
        perry_runtime::js_string_from_bytes(second_name.as_ptr(), second_name.len() as u32);
    js_object_set_field_by_name(
        obj,
        first_key,
        f64::from_bits(JSValue::pointer(first as *const u8).bits()),
    );
    js_object_set_field_by_name(
        obj,
        second_key,
        f64::from_bits(JSValue::pointer(second as *const u8).bits()),
    );
    resolve_with_bits(JSValue::pointer(obj as *const u8).bits())
}

unsafe fn import_shared_secret_key(
    key_bytes: &[u8],
    algo_bits: u64,
    extractable_bits: u64,
    usages_bits: u64,
) -> Result<*mut BufferHeader, *mut Promise> {
    let extractable = bool_from_jsvalue(extractable_bits);
    let Some(name) = extract_algo_name(algo_bits) else {
        return Err(reject_with_dom_exception(
            "NotSupportedError",
            "Unrecognized algorithm name",
        ));
    };
    let (key_algo, hash) = match name.to_ascii_uppercase().as_str() {
        "HMAC" => {
            let hash = match extract_hmac_hash(algo_bits) {
                Some(h) => h,
                None => {
                    return Err(reject_with_dom_exception(
                        "OperationError",
                        "The operation failed",
                    ))
                }
            };
            (KeyAlgo::Hmac, hash)
        }
        "AES-GCM" => (KeyAlgo::AesGcm, HashAlgo::Sha256),
        "AES-KW" => (KeyAlgo::AesKw, HashAlgo::Sha256),
        "AES-CBC" => (KeyAlgo::AesCbc, HashAlgo::Sha256),
        "AES-CTR" => (KeyAlgo::AesCtr, HashAlgo::Sha256),
        "AES-OCB" => (KeyAlgo::AesOcb, HashAlgo::Sha256),
        _ => {
            return Err(reject_with_dom_exception(
                "OperationError",
                "The operation failed",
            ))
        }
    };
    let usages = match validate_key_usages(
        key_algo,
        KeyKind::Secret,
        usages_bits,
        false,
        "Usages cannot be empty when importing a secret key.",
        "Unsupported key usage for the requested algorithm",
    ) {
        Ok(u) => u,
        Err((name, message)) => return Err(reject_with_dom_exception(name, message)),
    };
    let buf = alloc_uint8array_from_slice(key_bytes);
    if buf.is_null() {
        return Err(reject_with_dom_exception(
            "OperationError",
            "The operation failed",
        ));
    }
    register_crypto_key(
        buf as usize,
        CryptoKeyMaterial::new(key_algo, hash, KeyKind::Secret, extractable, usages),
    );
    Ok(buf)
}

unsafe fn object_with_ciphertext_and_key(
    ciphertext: &[u8],
    key: *mut BufferHeader,
) -> *mut Promise {
    let ciphertext_buf = alloc_uint8array_from_slice(ciphertext);
    if ciphertext_buf.is_null() || key.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let obj = js_object_alloc(0, 2);
    if obj.is_null() {
        return reject_with_dom_exception("OperationError", "The operation failed");
    }
    let ciphertext_key = perry_runtime::js_string_from_bytes(b"ciphertext".as_ptr(), 10);
    let shared_key = perry_runtime::js_string_from_bytes(b"sharedKey".as_ptr(), 9);
    js_object_set_field_by_name(
        obj,
        ciphertext_key,
        f64::from_bits(JSValue::pointer(ciphertext_buf as *const u8).bits()),
    );
    js_object_set_field_by_name(
        obj,
        shared_key,
        f64::from_bits(JSValue::pointer(key as *const u8).bits()),
    );
    resolve_with_bits(JSValue::pointer(obj as *const u8).bits())
}

pub unsafe fn js_webcrypto_encapsulate_bits(args_ptr: *const f64, args_len: usize) -> *mut Promise {
    if let Some(promise) = check_required_args("encapsulateBits", 2, args_len) {
        return promise;
    }
    let algo = match ml_kem_algo_arg(nth_arg(args_ptr, args_len, 0).to_bits()) {
        Ok(algo) => algo,
        Err(promise) => return promise,
    };
    let public_bits = nth_arg(args_ptr, args_len, 1).to_bits();
    let public_bytes = match key_material_arg(
        public_bits,
        algo,
        KeyKind::Public,
        USAGE_ENCAPSULATE_BITS,
        "encapsulationKey does not have encapsulateBits usage",
        "Failed to execute 'encapsulateBits' on 'SubtleCrypto': 2nd argument is not of type CryptoKey.",
    ) {
        Ok(bytes) => bytes,
        Err(promise) => return promise,
    };
    let (ciphertext, shared) = match ml_kem_encapsulate(algo, &public_bytes) {
        Some(result) => result,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    object_with_two_buffers(b"sharedKey", &shared, b"ciphertext", &ciphertext)
}

pub unsafe fn js_webcrypto_decapsulate_bits(args_ptr: *const f64, args_len: usize) -> *mut Promise {
    if let Some(promise) = check_required_args("decapsulateBits", 3, args_len) {
        return promise;
    }
    let algo = match ml_kem_algo_arg(nth_arg(args_ptr, args_len, 0).to_bits()) {
        Ok(algo) => algo,
        Err(promise) => return promise,
    };
    let private_bits = nth_arg(args_ptr, args_len, 1).to_bits();
    let private_bytes = match key_material_arg(
        private_bits,
        algo,
        KeyKind::Private,
        USAGE_DECAPSULATE_BITS,
        "decapsulationKey does not have decapsulateBits usage",
        "Failed to execute 'decapsulateBits' on 'SubtleCrypto': 2nd argument is not of type CryptoKey.",
    ) {
        Ok(bytes) => bytes,
        Err(promise) => return promise,
    };
    let ciphertext = bytes_from_jsvalue(nth_arg(args_ptr, args_len, 2).to_bits());
    let shared = match ml_kem_decapsulate(algo, &private_bytes, &ciphertext) {
        Some(shared) => shared,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    resolve_with_bytes(&shared)
}

pub unsafe fn js_webcrypto_encapsulate_key(args_ptr: *const f64, args_len: usize) -> *mut Promise {
    if let Some(promise) = check_required_args("encapsulateKey", 5, args_len) {
        return promise;
    }
    let algo = match ml_kem_algo_arg(nth_arg(args_ptr, args_len, 0).to_bits()) {
        Ok(algo) => algo,
        Err(promise) => return promise,
    };
    let public_bits = nth_arg(args_ptr, args_len, 1).to_bits();
    let public_bytes = match key_material_arg(
        public_bits,
        algo,
        KeyKind::Public,
        USAGE_ENCAPSULATE_KEY,
        "encapsulationKey does not have encapsulateKey usage",
        "Failed to execute 'encapsulateKey' on 'SubtleCrypto': 2nd argument is not of type CryptoKey.",
    ) {
        Ok(bytes) => bytes,
        Err(promise) => return promise,
    };
    let (ciphertext, shared) = match ml_kem_encapsulate(algo, &public_bytes) {
        Some(result) => result,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    let shared_key = match import_shared_secret_key(
        &shared,
        nth_arg(args_ptr, args_len, 2).to_bits(),
        nth_arg(args_ptr, args_len, 3).to_bits(),
        nth_arg(args_ptr, args_len, 4).to_bits(),
    ) {
        Ok(key) => key,
        Err(promise) => return promise,
    };
    object_with_ciphertext_and_key(&ciphertext, shared_key)
}

pub unsafe fn js_webcrypto_decapsulate_key(args_ptr: *const f64, args_len: usize) -> *mut Promise {
    if let Some(promise) = check_required_args("decapsulateKey", 6, args_len) {
        return promise;
    }
    let algo = match ml_kem_algo_arg(nth_arg(args_ptr, args_len, 0).to_bits()) {
        Ok(algo) => algo,
        Err(promise) => return promise,
    };
    let private_bits = nth_arg(args_ptr, args_len, 1).to_bits();
    let private_bytes = match key_material_arg(
        private_bits,
        algo,
        KeyKind::Private,
        USAGE_DECAPSULATE_KEY,
        "decapsulationKey does not have decapsulateKey usage",
        "Failed to execute 'decapsulateKey' on 'SubtleCrypto': 2nd argument is not of type CryptoKey.",
    ) {
        Ok(bytes) => bytes,
        Err(promise) => return promise,
    };
    let ciphertext = bytes_from_jsvalue(nth_arg(args_ptr, args_len, 2).to_bits());
    let shared = match ml_kem_decapsulate(algo, &private_bytes, &ciphertext) {
        Some(shared) => shared,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    let shared_key = match import_shared_secret_key(
        &shared,
        nth_arg(args_ptr, args_len, 3).to_bits(),
        nth_arg(args_ptr, args_len, 4).to_bits(),
        nth_arg(args_ptr, args_len, 5).to_bits(),
    ) {
        Ok(key) => key,
        Err(promise) => return promise,
    };
    resolve_with_bits(JSValue::pointer(shared_key as *const u8).bits())
}
