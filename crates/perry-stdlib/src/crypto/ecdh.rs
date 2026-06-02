use super::*;

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_sign(alg_ptr: i64) -> f64 {
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = match normalize_sign_algorithm(&alg_bytes) {
        Some(alg) => alg,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let handle: Handle = register_handle(SignHandle {
        alg,
        data: std::sync::Mutex::new(Vec::new()),
        finalized: std::sync::atomic::AtomicBool::new(false),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_verify(alg_ptr: i64) -> f64 {
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = match normalize_sign_algorithm(&alg_bytes) {
        Some(alg) => alg,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let handle: Handle = register_handle(VerifyHandle {
        alg,
        data: std::sync::Mutex::new(Vec::new()),
        finalized: std::sync::atomic::AtomicBool::new(false),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_ecdh(curve_ptr: i64) -> f64 {
    let curve = String::from_utf8(bytes_from_ptr(curve_ptr))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(curve.as_str(), "prime256v1" | "secp256r1" | "p-256") {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }
    let handle: Handle = register_handle(EcdhHandle {
        private_key: std::sync::Mutex::new(None),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_diffie_hellman(
    prime_val: f64,
    second_val: f64,
    third_val: f64,
) -> f64 {
    let second_string = if second_val.is_finite() {
        String::new()
    } else {
        String::from_utf8(bytes_from_ptr(arg_ptr(second_val))).unwrap_or_default()
    };
    let (prime_encoding, generator_value, generator_encoding) = if matches!(
        second_string.as_str(),
        "hex" | "base64" | "buffer" | "latin1" | "binary"
    ) {
        (
            second_string.as_str(),
            if third_val.to_bits() == JSValue::undefined().bits() {
                None
            } else {
                Some(third_val)
            },
            second_string.as_str(),
        )
    } else {
        (
            "",
            if second_val.to_bits() == JSValue::undefined().bits() {
                None
            } else {
                Some(second_val)
            },
            "",
        )
    };

    let prime = decode_dh_prime_value(prime_val, prime_encoding);
    let generator = decode_dh_generator_value(generator_value, generator_encoding);
    let handle: Handle = register_handle(DiffieHellmanHandle {
        prime,
        generator,
        private_key: std::sync::Mutex::new(None),
        public_key: std::sync::Mutex::new(None),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_get_diffie_hellman(_group_val: f64) -> f64 {
    let handle: Handle = register_handle(DiffieHellmanHandle {
        prime: dh_default_prime(),
        generator: dh_default_generator(),
        private_key: std::sync::Mutex::new(None),
        public_key: std::sync::Mutex::new(None),
    });
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_ecdh_convert_key(
    key_val: f64,
    curve_val: f64,
    input_encoding_val: f64,
    output_encoding_val: f64,
    format_val: f64,
) -> f64 {
    let curve_ptr = arg_ptr(curve_val);
    let curve = String::from_utf8(bytes_from_ptr(curve_ptr))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(curve.as_str(), "prime256v1" | "secp256r1" | "p-256") {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }

    let input_encoding =
        String::from_utf8(bytes_from_ptr(arg_ptr(input_encoding_val))).unwrap_or_default();
    let output_encoding =
        String::from_utf8(bytes_from_ptr(arg_ptr(output_encoding_val))).unwrap_or_default();
    let format = String::from_utf8(bytes_from_ptr(arg_ptr(format_val))).unwrap_or_default();
    let key_bytes = decode_ecdh_input(arg_ptr(key_val), &input_encoding);
    let public = match P256PublicKey::from_sec1_bytes(&key_bytes) {
        Ok(public) => public,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    let compressed = format.eq_ignore_ascii_case("compressed");
    let converted = public.to_encoded_point(compressed).as_bytes().to_vec();
    ecdh_output(
        &converted,
        (!output_encoding.is_empty()).then_some(output_encoding.as_str()),
    )
}

pub unsafe fn dispatch_sign(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<SignHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    // #2963 — once `.sign()` consumed the handle, Node throws on any further
    // `update`/`sign`. Guard both terminal-affecting methods.
    if matches!(method, "update" | "sign") && h.finalized.load(std::sync::atomic::Ordering::Relaxed)
    {
        perry_runtime::fs::validate::throw_error_with_code(
            "Not initialised",
            "ERR_CRYPTO_INVALID_STATE",
        );
    }
    match method {
        "update" if !args.is_empty() => {
            let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let bytes = bytes_from_ptr(ptr);
            h.data.lock().unwrap().extend_from_slice(&bytes);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        "sign" if !args.is_empty() => {
            // The handle is consumed by `.sign()` regardless of outcome.
            h.finalized
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let key_bits = args[0].to_bits();
            let pem = match crypto_key_input_to_private_pem(key_bits) {
                Some(pem) => pem,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            if let Some(signing_key) = parse_p256_signing_key_pem(&pem) {
                let data = h.data.lock().unwrap().clone();
                let signature: P256EcdsaSignature = signing_key.sign(&data);
                if key_input_uses_ieee_p1363(key_bits) {
                    let raw = signature.to_bytes();
                    let buf = alloc_buffer_from_slice(raw.as_slice());
                    return f64::from_bits(
                        0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF),
                    );
                }
                let der = signature.to_der();
                let buf = alloc_buffer_from_slice(der.as_bytes());
                return f64::from_bits(
                    0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF),
                );
            }
            let private_key = match parse_rsa_private_key_pem(&pem) {
                Some(key) => key,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let data = h.data.lock().unwrap().clone();
            let signature = if key_input_uses_rsa_pss(key_bits) {
                let salt_len = key_input_pss_salt_len(key_bits, h.alg);
                sign_rsa_pss_data(h.alg, private_key, &data, salt_len)
            } else {
                sign_rsa_data(h.alg, private_key, &data)
            };
            let buf = alloc_buffer_from_slice(&signature);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((buf as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_sign_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "update" => b"update",
        "sign" => b"sign",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}

pub unsafe fn dispatch_ecdh(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<EcdhHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    match method {
        "generateKeys" | "dhGenerateKeys" => {
            let key = match generate_p256_secret_key() {
                Some(key) => key,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let encoding = arg_string(args, 0);
            let format = arg_string(args, 1);
            let public = p256_public_bytes(&key, &format);
            *h.private_key.lock().unwrap() = Some(key);
            ecdh_output(&public, (!encoding.is_empty()).then_some(encoding.as_str()))
        }
        "getPublicKey" => {
            let guard = h.private_key.lock().unwrap();
            let key = match guard.as_ref() {
                Some(key) => key,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let encoding = arg_string(args, 0);
            let format = arg_string(args, 1);
            let public = p256_public_bytes(key, &format);
            ecdh_output(&public, (!encoding.is_empty()).then_some(encoding.as_str()))
        }
        "getPrivateKey" | "dhGetPrivateKey" => {
            let guard = h.private_key.lock().unwrap();
            let key = match guard.as_ref() {
                Some(key) => key,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let encoding = arg_string(args, 0);
            let bytes = key.to_bytes();
            ecdh_output(
                bytes.as_slice(),
                (!encoding.is_empty()).then_some(encoding.as_str()),
            )
        }
        "setPrivateKey" if !args.is_empty() => {
            let encoding = arg_string(args, 1);
            let mut bytes = arg_bytes(args, 0);
            if encoding.eq_ignore_ascii_case("hex") {
                let s = String::from_utf8(bytes).unwrap_or_default();
                bytes = hex::decode(s).unwrap_or_default();
            }
            match P256SecretKey::from_slice(&bytes) {
                Ok(key) => {
                    *h.private_key.lock().unwrap() = Some(key);
                    f64::from_bits(JSValue::undefined().bits())
                }
                Err(_) => f64::from_bits(0x7FFC_0000_0000_0001),
            }
        }
        // "deprecated" is the bound-method alias for setPublicKey (#1368).
        "setPublicKey" | "deprecated" => f64::from_bits(JSValue::undefined().bits()),
        "computeSecret" | "dhComputeSecret" if !args.is_empty() => {
            let guard = h.private_key.lock().unwrap();
            let key = match guard.as_ref() {
                Some(key) => key,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let input_encoding = arg_string(args, 1);
            let output_encoding = arg_string(args, 2);
            let public_bytes = decode_ecdh_input(arg_ptr(args[0]), &input_encoding);
            let public = match P256PublicKey::from_sec1_bytes(&public_bytes) {
                Ok(public) => public,
                Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            let secret = p256_diffie_hellman(key.to_nonzero_scalar(), public.as_affine());
            ecdh_output(
                secret.raw_secret_bytes().as_slice(),
                (!output_encoding.is_empty()).then_some(output_encoding.as_str()),
            )
        }
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_ecdh_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "generateKeys" => b"generateKeys",
        "getPublicKey" => b"getPublicKey",
        "getPrivateKey" => b"dhGetPrivateKey",
        "setPrivateKey" => b"setPrivateKey",
        // #1368: Node deprecated ECDH.setPublicKey (DEP0031) and exposes it
        // via a `deprecate()` wrapper, so its `.name` is "deprecated".
        // `dispatch_ecdh` accepts "deprecated" as a setPublicKey alias so a
        // captured-then-called `const f = ecdh.setPublicKey; f(...)` still
        // dispatches. (DH's setPublicKey is NOT deprecated — keeps its name.)
        "setPublicKey" => b"deprecated",
        "computeSecret" => b"dhComputeSecret",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}

pub unsafe fn dispatch_diffie_hellman(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<DiffieHellmanHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    match method {
        "generateKeys" => {
            let encoding = arg_string(args, 0);
            let private = {
                let mut private_guard = h.private_key.lock().unwrap();
                match private_guard.as_ref() {
                    Some(private) => private.clone(),
                    None => {
                        let private = dh_random_private_key(&h.prime);
                        *private_guard = Some(private.clone());
                        private
                    }
                }
            };
            let public = dh_public_from_private(&h.prime, &h.generator, &private);
            *h.public_key.lock().unwrap() = Some(public.clone());
            ecdh_output(&public, (!encoding.is_empty()).then_some(encoding.as_str()))
        }
        "computeSecret" | "dhComputeSecret" if !args.is_empty() => {
            let input_encoding = arg_string(args, 1);
            let output_encoding = arg_string(args, 2);
            let other_public = decode_crypto_value(args[0], &input_encoding);
            let private = {
                let mut private_guard = h.private_key.lock().unwrap();
                match private_guard.as_ref() {
                    Some(private) => private.clone(),
                    None => {
                        let private = dh_random_private_key(&h.prime);
                        let public = dh_public_from_private(&h.prime, &h.generator, &private);
                        *h.public_key.lock().unwrap() = Some(public);
                        *private_guard = Some(private.clone());
                        private
                    }
                }
            };
            let secret = dh_secret(&h.prime, &private, &other_public);
            ecdh_output(
                &secret,
                (!output_encoding.is_empty()).then_some(output_encoding.as_str()),
            )
        }
        "getPrime" | "dhGetPrime" => {
            let encoding = arg_string(args, 0);
            ecdh_output(
                &h.prime,
                (!encoding.is_empty()).then_some(encoding.as_str()),
            )
        }
        "getGenerator" | "dhGetGenerator" => {
            let encoding = arg_string(args, 0);
            ecdh_output(
                &h.generator,
                (!encoding.is_empty()).then_some(encoding.as_str()),
            )
        }
        "getPublicKey" | "dhGetPublicKey" => {
            let encoding = arg_string(args, 0);
            let public = {
                let public_guard = h.public_key.lock().unwrap();
                public_guard.as_ref().cloned()
            }
            .or_else(|| {
                h.private_key
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|private| dh_public_from_private(&h.prime, &h.generator, private))
            })
            .unwrap_or_default();
            ecdh_output(&public, (!encoding.is_empty()).then_some(encoding.as_str()))
        }
        "getPrivateKey" | "dhGetPrivateKey" => {
            let encoding = arg_string(args, 0);
            let private = h
                .private_key
                .lock()
                .unwrap()
                .as_ref()
                .cloned()
                .unwrap_or_default();
            ecdh_output(
                &private,
                (!encoding.is_empty()).then_some(encoding.as_str()),
            )
        }
        "setPrivateKey" if !args.is_empty() => {
            let encoding = arg_string(args, 1);
            let private = decode_crypto_value(args[0], &encoding);
            *h.private_key.lock().unwrap() = Some(private);
            f64::from_bits(JSValue::undefined().bits())
        }
        "setPublicKey" if !args.is_empty() => {
            let encoding = arg_string(args, 1);
            let public = decode_crypto_value(args[0], &encoding);
            *h.public_key.lock().unwrap() = Some(public);
            f64::from_bits(JSValue::undefined().bits())
        }
        "verifyError" => 0.0,
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_diffie_hellman_property(handle: i64, property: &str) -> f64 {
    if property == "verifyError" {
        return 0.0;
    }
    let name_bytes: &'static [u8] = match property {
        "generateKeys" => b"dhGenerateKeys",
        "computeSecret" => b"dhComputeSecret",
        "getPrime" => b"dhGetPrime",
        "getGenerator" => b"dhGetGenerator",
        "getPublicKey" => b"dhGetPublicKey",
        "getPrivateKey" => b"dhGetPrivateKey",
        "setPublicKey" => b"setPublicKey",
        "setPrivateKey" => b"setPrivateKey",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}

pub unsafe fn dispatch_verify(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<VerifyHandle>(handle) {
        Some(h) => h,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };
    // #2963 — once `.verify()` consumed the handle, Node throws on any further
    // `update`/`verify`.
    if matches!(method, "update" | "verify")
        && h.finalized.load(std::sync::atomic::Ordering::Relaxed)
    {
        perry_runtime::fs::validate::throw_error_with_code(
            "Not initialised",
            "ERR_CRYPTO_INVALID_STATE",
        );
    }
    match method {
        "update" if !args.is_empty() => {
            let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let bytes = bytes_from_ptr(ptr);
            h.data.lock().unwrap().extend_from_slice(&bytes);
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((handle as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
        "verify" if args.len() >= 2 => {
            // The handle is consumed by `.verify()` regardless of outcome.
            h.finalized
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let key_bits = args[0].to_bits();
            let sig_ptr = (args[1].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let pem = match crypto_key_input_to_public_pem(key_bits) {
                Some(pem) => pem,
                None => return js_bool(false),
            };
            if let Some(verifying_key) = parse_p256_verifying_key_pem(&pem) {
                let sig_bytes = bytes_from_ptr(sig_ptr);
                let signature = if key_input_uses_ieee_p1363(key_bits) {
                    P256EcdsaSignature::from_slice(&sig_bytes)
                } else {
                    P256EcdsaSignature::from_der(&sig_bytes)
                };
                let signature = match signature {
                    Ok(sig) => sig,
                    Err(_) => return js_bool(false),
                };
                let data = h.data.lock().unwrap().clone();
                return js_bool(verifying_key.verify(&data, &signature).is_ok());
            }
            let public_key = match parse_rsa_public_key_pem(&pem) {
                Some(key) => key,
                None => return js_bool(false),
            };
            let sig_bytes = bytes_from_ptr(sig_ptr);
            if key_input_uses_rsa_pss(key_bits) {
                let signature = match RsaPssSignature::try_from(sig_bytes.as_slice()) {
                    Ok(sig) => sig,
                    Err(_) => return js_bool(false),
                };
                let data = h.data.lock().unwrap().clone();
                let salt_len = key_input_pss_salt_len(key_bits, h.alg);
                return js_bool(verify_rsa_pss_data(
                    h.alg, public_key, &data, &signature, salt_len,
                ));
            }
            let signature = match RsaPkcs1v15Signature::try_from(sig_bytes.as_slice()) {
                Ok(sig) => sig,
                Err(_) => return js_bool(false),
            };
            let data = h.data.lock().unwrap().clone();
            js_bool(verify_rsa_data(h.alg, public_key, &data, &signature))
        }
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    }
}

pub unsafe fn dispatch_verify_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "update" => b"update",
        "verify" => b"verify",
        _ => return nanbox_undefined(),
    };
    let this_f64 = nanbox_pointer_f64(handle as usize);
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(this_f64, name_bytes.as_ptr(), name_bytes.len())
}
