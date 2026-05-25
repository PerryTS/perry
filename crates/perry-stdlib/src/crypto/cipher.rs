use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CipherKind {
    Aes128Cbc,
    Aes192Cbc,
    Aes256Cbc,
    Aes128Ecb,
    Aes192Ecb,
    Aes256Ecb,
    Aes128Wrap,
    Aes192Wrap,
    Aes256Wrap,
    Aes128Gcm,
    Aes192Gcm,
    Aes256Gcm,
}

impl CipherKind {
    fn parse(alg: &str) -> Option<Self> {
        match alg.to_ascii_lowercase().as_str() {
            "aes-128-cbc" => Some(Self::Aes128Cbc),
            "aes-192-cbc" => Some(Self::Aes192Cbc),
            "aes-256-cbc" => Some(Self::Aes256Cbc),
            "aes-128-ecb" => Some(Self::Aes128Ecb),
            "aes-192-ecb" => Some(Self::Aes192Ecb),
            "aes-256-ecb" => Some(Self::Aes256Ecb),
            "id-aes128-wrap" | "aes128-wrap" => Some(Self::Aes128Wrap),
            "id-aes192-wrap" | "aes192-wrap" => Some(Self::Aes192Wrap),
            "id-aes256-wrap" | "aes256-wrap" => Some(Self::Aes256Wrap),
            "aes-128-gcm" => Some(Self::Aes128Gcm),
            "aes-192-gcm" => Some(Self::Aes192Gcm),
            "aes-256-gcm" => Some(Self::Aes256Gcm),
            _ => None,
        }
    }

    fn key_len(self) -> usize {
        match self {
            Self::Aes128Cbc | Self::Aes128Ecb | Self::Aes128Wrap | Self::Aes128Gcm => 16,
            Self::Aes192Cbc | Self::Aes192Ecb | Self::Aes192Wrap | Self::Aes192Gcm => 24,
            Self::Aes256Cbc | Self::Aes256Ecb | Self::Aes256Wrap | Self::Aes256Gcm => 32,
        }
    }

    fn is_gcm(self) -> bool {
        matches!(self, Self::Aes128Gcm | Self::Aes192Gcm | Self::Aes256Gcm)
    }

    fn is_ecb(self) -> bool {
        matches!(self, Self::Aes128Ecb | Self::Aes192Ecb | Self::Aes256Ecb)
    }

    fn is_wrap(self) -> bool {
        matches!(self, Self::Aes128Wrap | Self::Aes192Wrap | Self::Aes256Wrap)
    }
}

// CBC type aliases (Aes256CbcEnc/Dec already exist above for aes-256-cbc).
pub(super) type Aes128CbcEnc = Encryptor<Aes128>;
pub(super) type Aes128CbcDec = Decryptor<Aes128>;
pub(super) type Aes192CbcEnc = Encryptor<Aes192>;
pub(super) type Aes192CbcDec = Decryptor<Aes192>;
pub(super) type Aes128EcbEnc = ecb::Encryptor<Aes128>;
pub(super) type Aes128EcbDec = ecb::Decryptor<Aes128>;
pub(super) type Aes192EcbEnc = ecb::Encryptor<Aes192>;
pub(super) type Aes192EcbDec = ecb::Decryptor<Aes192>;
pub(super) type Aes256EcbEnc = ecb::Encryptor<Aes256>;
pub(super) type Aes256EcbDec = ecb::Decryptor<Aes256>;
pub(super) type Aes192Gcm = aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12>;
pub(super) type Aes128Gcm12 =
    aes_gcm::AesGcm<Aes128, aes::cipher::consts::U12, aes::cipher::consts::U12>;
pub(super) type Aes128Gcm13 =
    aes_gcm::AesGcm<Aes128, aes::cipher::consts::U12, aes::cipher::consts::U13>;
pub(super) type Aes128Gcm14 =
    aes_gcm::AesGcm<Aes128, aes::cipher::consts::U12, aes::cipher::consts::U14>;
pub(super) type Aes128Gcm15 =
    aes_gcm::AesGcm<Aes128, aes::cipher::consts::U12, aes::cipher::consts::U15>;
pub(super) type Aes192Gcm12 =
    aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12, aes::cipher::consts::U12>;
pub(super) type Aes192Gcm13 =
    aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12, aes::cipher::consts::U13>;
pub(super) type Aes192Gcm14 =
    aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12, aes::cipher::consts::U14>;
pub(super) type Aes192Gcm15 =
    aes_gcm::AesGcm<Aes192, aes::cipher::consts::U12, aes::cipher::consts::U15>;
pub(super) type Aes256Gcm12 =
    aes_gcm::AesGcm<Aes256, aes::cipher::consts::U12, aes::cipher::consts::U12>;
pub(super) type Aes256Gcm13 =
    aes_gcm::AesGcm<Aes256, aes::cipher::consts::U12, aes::cipher::consts::U13>;
pub(super) type Aes256Gcm14 =
    aes_gcm::AesGcm<Aes256, aes::cipher::consts::U12, aes::cipher::consts::U14>;
pub(super) type Aes256Gcm15 =
    aes_gcm::AesGcm<Aes256, aes::cipher::consts::U12, aes::cipher::consts::U15>;

pub(super) type Aes128Ctr32Be = ctr::Ctr32BE<Aes128>;
pub(super) type Aes192Ctr32Be = ctr::Ctr32BE<Aes192>;
pub(super) type Aes256Ctr32Be = ctr::Ctr32BE<Aes256>;

pub(super) fn aes_encrypt_block<C>(key: &[u8], block: [u8; 16]) -> Option<[u8; 16]>
where
    C: aes::cipher::BlockEncrypt + aes::cipher::KeyInit,
{
    let cipher = <C as aes::cipher::KeyInit>::new_from_slice(key).ok()?;
    let mut block = aes::cipher::generic_array::GenericArray::clone_from_slice(&block);
    cipher.encrypt_block(&mut block);
    let mut out = [0u8; 16];
    out.copy_from_slice(&block);
    Some(out)
}

pub(super) fn gcm_ghash<C>(key: &[u8], aad: &[u8], ciphertext: &[u8]) -> Option<[u8; 16]>
where
    C: aes::cipher::BlockEncrypt + aes::cipher::KeyInit,
{
    use ghash::universal_hash::{KeyInit as GHashKeyInit, UniversalHash};

    let h = aes_encrypt_block::<C>(key, [0u8; 16])?;
    let mut ghash = ghash::GHash::new(ghash::Key::from_slice(&h));
    ghash.update_padded(aad);
    ghash.update_padded(ciphertext);

    let mut lengths = [0u8; 16];
    lengths[..8].copy_from_slice(&((aad.len() as u64).wrapping_mul(8)).to_be_bytes());
    lengths[8..].copy_from_slice(&((ciphertext.len() as u64).wrapping_mul(8)).to_be_bytes());
    let length_block = ghash::Block::clone_from_slice(&lengths);
    ghash.update(std::slice::from_ref(&length_block));

    let tag = ghash.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&tag);
    Some(out)
}

pub(super) fn gcm_j0<C>(key: &[u8], iv: &[u8]) -> Option<[u8; 16]>
where
    C: aes::cipher::BlockEncrypt + aes::cipher::KeyInit,
{
    if iv.len() == 12 {
        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(iv);
        j0[15] = 1;
        return Some(j0);
    }

    // For non-96-bit IVs, GCM derives J0 with GHASH over the IV and a
    // length block. This is equivalent to GHASH with empty AAD and IV as
    // the ciphertext input.
    gcm_ghash::<C>(key, &[], iv)
}

pub(super) fn gcm_inc32(mut block: [u8; 16]) -> [u8; 16] {
    let counter = u32::from_be_bytes([block[12], block[13], block[14], block[15]]).wrapping_add(1);
    block[12..].copy_from_slice(&counter.to_be_bytes());
    block
}

pub(super) fn gcm_tag<C>(
    key: &[u8],
    j0: [u8; 16],
    aad: &[u8],
    ciphertext: &[u8],
) -> Option<[u8; 16]>
where
    C: aes::cipher::BlockEncrypt + aes::cipher::KeyInit,
{
    let mut tag = gcm_ghash::<C>(key, aad, ciphertext)?;
    let mask = aes_encrypt_block::<C>(key, j0)?;
    for (tag_byte, mask_byte) in tag.iter_mut().zip(mask.iter()) {
        *tag_byte ^= mask_byte;
    }
    Some(tag)
}

pub(super) fn tag_prefix_eq(actual: &[u8], expected: &[u8]) -> bool {
    if actual.len() > expected.len() {
        return false;
    }
    actual
        .iter()
        .zip(expected.iter())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

pub(super) fn decrypt_gcm_short_tag<C, S>(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8],
) -> Option<Vec<u8>>
where
    C: aes::cipher::BlockEncrypt + aes::cipher::KeyInit,
    S: aes::cipher::KeyIvInit + aes::cipher::StreamCipher,
{
    let j0 = gcm_j0::<C>(key, iv)?;
    let counter = gcm_inc32(j0);
    let mut plaintext = ciphertext.to_vec();
    <S as aes::cipher::KeyIvInit>::new_from_slices(key, &counter)
        .ok()?
        .apply_keystream(&mut plaintext);

    let expected_tag = gcm_tag::<C>(key, j0, aad, ciphertext)?;
    tag_prefix_eq(tag, &expected_tag).then_some(plaintext)
}

pub(super) fn decrypt_gcm128_with_tag_len(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8],
) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes128Gcm, Nonce};
    if matches!(tag.len(), 4 | 8) {
        return decrypt_gcm_short_tag::<Aes128, Aes128Ctr32Be>(key, iv, aad, ciphertext, tag);
    }
    let nonce = Nonce::from_slice(iv);
    let mut combined = ciphertext.to_vec();
    combined.extend_from_slice(tag);
    match tag.len() {
        12 => Aes128Gcm12::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        13 => Aes128Gcm13::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        14 => Aes128Gcm14::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        15 => Aes128Gcm15::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        16 => Aes128Gcm::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        _ => None,
    }
}

pub(super) fn decrypt_gcm192_with_tag_len(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8],
) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::Nonce;
    if matches!(tag.len(), 4 | 8) {
        return decrypt_gcm_short_tag::<Aes192, Aes192Ctr32Be>(key, iv, aad, ciphertext, tag);
    }
    let nonce = Nonce::from_slice(iv);
    let mut combined = ciphertext.to_vec();
    combined.extend_from_slice(tag);
    match tag.len() {
        12 => Aes192Gcm12::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        13 => Aes192Gcm13::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        14 => Aes192Gcm14::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        15 => Aes192Gcm15::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        16 => Aes192Gcm::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        _ => None,
    }
}

pub(super) fn decrypt_gcm256_with_tag_len(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8],
) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    if matches!(tag.len(), 4 | 8) {
        return decrypt_gcm_short_tag::<Aes256, Aes256Ctr32Be>(key, iv, aad, ciphertext, tag);
    }
    let nonce = Nonce::from_slice(iv);
    let mut combined = ciphertext.to_vec();
    combined.extend_from_slice(tag);
    match tag.len() {
        12 => Aes256Gcm12::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        13 => Aes256Gcm13::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        14 => Aes256Gcm14::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        15 => Aes256Gcm15::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        16 => Aes256Gcm::new_from_slice(key)
            .ok()?
            .decrypt(
                nonce,
                Payload {
                    msg: &combined,
                    aad,
                },
            )
            .ok(),
        _ => None,
    }
}

/// Per-handle cipher state. CBC ciphers accumulate plaintext (or
/// ciphertext, on decrypt) in `buffer` until `.final()` runs the
/// single-shot encryptor/decryptor — block ciphers can't safely emit
/// partial output without buffering the trailing fragment for PKCS7
/// padding anyway, and bouncing through the `_padded_mut` API keeps
/// the implementation small. GCM uses `aes_gcm::Aes256Gcm::encrypt`
/// / `decrypt` which are one-shot AEAD ops, so the same "buffer and
/// flush on final" shape applies there.
pub struct CipherHandle {
    state: std::sync::Mutex<CipherState>,
}

pub(super) struct CipherState {
    kind: CipherKind,
    encrypt: bool,
    key: Vec<u8>,
    iv: Vec<u8>,
    auth_tag_len: usize,
    buffer: Vec<u8>,
    /// For GCM encrypt: filled in by `.final()`, read by `.getAuthTag()`.
    /// For GCM decrypt: set by `.setAuthTag(tag)` and consumed at `.final()`.
    auth_tag: Option<Vec<u8>>,
    aad: Vec<u8>,
    auto_padding: bool,
    finished: bool,
}

#[inline]
pub(super) fn nanbox_pointer_f64(ptr: usize) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((ptr as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[inline]
pub(super) fn nanbox_undefined() -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

pub(super) unsafe fn create_cipher_handle(
    alg_ptr: i64,
    key_ptr: i64,
    iv_ptr: i64,
    options_bits: f64,
    encrypt: bool,
) -> f64 {
    let alg_bytes = bytes_from_ptr(alg_ptr);
    let alg = std::str::from_utf8(&alg_bytes).unwrap_or("");
    let kind = match CipherKind::parse(alg) {
        Some(k) => k,
        None => return nanbox_undefined(),
    };
    let key = bytes_from_ptr(key_ptr);
    let iv = bytes_from_ptr(iv_ptr);
    if key.len() != kind.key_len() {
        return nanbox_undefined();
    }
    // GCM accepts a 12-byte nonce (recommended) or any non-empty IV; we
    // require 12 to match what Node verifies against the standard AES-GCM
    // implementations. CBC requires exactly 16 (one block).
    if kind.is_gcm() {
        if iv.is_empty() {
            return nanbox_undefined();
        }
    } else if kind.is_ecb() {
        if !iv.is_empty() {
            return nanbox_undefined();
        }
    } else if kind.is_wrap() {
        if iv.len() != 8 {
            return nanbox_undefined();
        }
    } else if iv.len() != 16 {
        return nanbox_undefined();
    }
    // Node permits GCM auth-tag lengths {4, 8, 12, 13, 14, 15, 16}; values
    // outside that set throw. The RustCrypto Aes*Gcm backend only natively
    // produces 12-16 byte tags, but the cipher state truncates the tag
    // down to `auth_tag_len` before `getAuthTag()` returns, so a request
    // for 4 / 8 still produces a tag with the expected length. Filter to
    // 1..=16 (Node-superset; out-of-range falls through to the default).
    let auth_tag_len = if kind.is_gcm() {
        object_field_bits(options_bits.to_bits(), b"authTagLength")
            .and_then(|bits| nanboxed_to_usize(f64::from_bits(bits)))
            .filter(|len| (1..=16).contains(len))
            .unwrap_or(16)
    } else {
        0
    };
    let handle: Handle = register_handle(CipherHandle {
        state: std::sync::Mutex::new(CipherState {
            kind,
            encrypt,
            key,
            iv,
            auth_tag_len,
            buffer: Vec::new(),
            auth_tag: None,
            aad: Vec::new(),
            auto_padding: true,
            finished: false,
        }),
    });
    nanbox_pointer_f64(handle as usize)
}

/// `crypto.createCipheriv(alg, key, iv)` — register a CipherHandle for
/// encryption and return its handle NaN-boxed as POINTER_TAG.
///
/// # Safety
/// Pointers must point at a Buffer or StringHeader (both layouts are
/// handled by `bytes_from_ptr`).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_cipheriv(
    alg_ptr: i64,
    key_ptr: i64,
    iv_ptr: i64,
    options_bits: f64,
) -> f64 {
    create_cipher_handle(alg_ptr, key_ptr, iv_ptr, options_bits, true)
}

/// `crypto.createDecipheriv(alg, key, iv)` — register a CipherHandle for
/// decryption and return its handle NaN-boxed as POINTER_TAG.
///
/// # Safety
/// Pointers must point at a Buffer or StringHeader (both layouts are
/// handled by `bytes_from_ptr`).
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_decipheriv(
    alg_ptr: i64,
    key_ptr: i64,
    iv_ptr: i64,
    options_bits: f64,
) -> f64 {
    create_cipher_handle(alg_ptr, key_ptr, iv_ptr, options_bits, false)
}

/// Dispatch `update` / `final` / `getAuthTag` / `setAuthTag` on a
/// CipherHandle. Called from `common/dispatch.rs::js_handle_method_dispatch`.
pub unsafe fn dispatch_cipher(handle: i64, method: &str, args: &[f64]) -> f64 {
    let h = match get_handle_mut::<CipherHandle>(handle) {
        Some(h) => h,
        None => return nanbox_undefined(),
    };
    let mut guard = h.state.lock().unwrap();
    let state = &mut *guard;
    match method {
        // `.update(buf)` — accumulate plaintext / ciphertext. Node returns
        // an incremental chunk here; for CBC/GCM we can safely return an
        // empty Buffer and emit everything at `.final()` because
        // `Buffer.concat([cipher.update(x), cipher.final()])` is what the
        // overwhelming majority of callers do. This matches `Buffer.concat`
        // length-wise (empty + total == total) and avoids the partial-block
        // bookkeeping that streaming CBC would need.
        "update" => {
            if state.finished {
                return nanbox_undefined();
            }
            if args.is_empty() {
                let buf = alloc_buffer_from_slice(&[]);
                return nanbox_pointer_f64(buf as usize);
            }
            let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let bytes = bytes_from_ptr(ptr);
            state.buffer.extend_from_slice(&bytes);
            let buf = alloc_buffer_from_slice(&[]);
            nanbox_pointer_f64(buf as usize)
        }
        // `.final()` — runs the actual encrypt/decrypt and returns the
        // full output. For GCM-encrypt this also stashes the 16-byte auth
        // tag in `auth_tag` for a subsequent `.getAuthTag()` call.
        "final" => {
            if state.finished {
                let buf = alloc_buffer_from_slice(&[]);
                return nanbox_pointer_f64(buf as usize);
            }
            state.finished = true;
            let plaintext_or_ct = std::mem::take(&mut state.buffer);
            let output: Vec<u8> = match (state.kind, state.encrypt) {
                (CipherKind::Aes256Cbc, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher = match Aes256CbcEnc::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Cbc, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher = match Aes128CbcEnc::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes192Cbc, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher = match Aes192CbcEnc::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes256Ecb, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher =
                        Aes256EcbEnc::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes192Ecb, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher =
                        Aes192EcbEnc::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Ecb, true) => {
                    let block_size = 16;
                    let padded_len = if state.auto_padding {
                        (plaintext_or_ct.len() / block_size + 1) * block_size
                    } else {
                        plaintext_or_ct.len()
                    };
                    let mut buf = vec![0u8; padded_len];
                    buf[..plaintext_or_ct.len()].copy_from_slice(&plaintext_or_ct);
                    let cipher =
                        Aes128EcbEnc::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext_or_ct.len()) {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher
                            .encrypt_padded_mut::<NoPadding>(&mut buf, plaintext_or_ct.len())
                        {
                            Ok(ct) => ct.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Wrap, true) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes128};
                    let kw =
                        KwAes128::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len() + 8];
                    match kw.wrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(ct) => ct.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes192Wrap, true) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes192};
                    let kw =
                        KwAes192::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len() + 8];
                    match kw.wrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(ct) => ct.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes256Wrap, true) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes256};
                    let kw =
                        KwAes256::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len() + 8];
                    match kw.wrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(ct) => ct.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes256Cbc, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher = match Aes256CbcDec::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Cbc, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher = match Aes128CbcDec::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes192Cbc, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher = match Aes192CbcDec::new_from_slices(&state.key, &state.iv) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes256Ecb, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher =
                        Aes256EcbDec::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes192Ecb, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher =
                        Aes192EcbDec::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Ecb, false) => {
                    let mut buf = plaintext_or_ct.clone();
                    let cipher =
                        Aes128EcbDec::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    if state.auto_padding {
                        match cipher.decrypt_padded_mut::<Pkcs7>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    } else {
                        match cipher.decrypt_padded_mut::<NoPadding>(&mut buf) {
                            Ok(pt) => pt.to_vec(),
                            Err(_) => return nanbox_undefined(),
                        }
                    }
                }
                (CipherKind::Aes128Wrap, false) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes128};
                    let kw =
                        KwAes128::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len().saturating_sub(8)];
                    match kw.unwrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(pt) => pt.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes192Wrap, false) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes192};
                    let kw =
                        KwAes192::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len().saturating_sub(8)];
                    match kw.unwrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(pt) => pt.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes256Wrap, false) => {
                    use aes_kw::{KeyInit as AesKwKeyInit, KwAes256};
                    let kw =
                        KwAes256::new_from_slice(&state.key).unwrap_or_else(|_| unreachable!());
                    let mut buf = vec![0u8; plaintext_or_ct.len().saturating_sub(8)];
                    match kw.unwrap_key(&plaintext_or_ct, &mut buf) {
                        Ok(pt) => pt.to_vec(),
                        Err(_) => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes256Gcm, true) => {
                    use aes_gcm::aead::{Aead, KeyInit, Payload};
                    use aes_gcm::{Aes256Gcm, Nonce};
                    let cipher = match Aes256Gcm::new_from_slice(&state.key) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    let nonce = Nonce::from_slice(&state.iv);
                    let payload = Payload {
                        msg: plaintext_or_ct.as_ref(),
                        aad: state.aad.as_ref(),
                    };
                    let mut ct = match cipher.encrypt(nonce, payload) {
                        Ok(ct) => ct,
                        Err(_) => return nanbox_undefined(),
                    };
                    // aes-gcm appends the 16-byte tag to the ciphertext.
                    // Node's createCipheriv splits these: update/final
                    // produces just the ciphertext, getAuthTag returns
                    // the tag separately.
                    let tag = ct.split_off(ct.len().saturating_sub(16));
                    state.auth_tag = Some(tag[..state.auth_tag_len.min(tag.len())].to_vec());
                    ct
                }
                (CipherKind::Aes128Gcm, true) => {
                    use aes_gcm::aead::{Aead, KeyInit, Payload};
                    use aes_gcm::{Aes128Gcm, Nonce};
                    let cipher = match Aes128Gcm::new_from_slice(&state.key) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    let nonce = Nonce::from_slice(&state.iv);
                    let payload = Payload {
                        msg: plaintext_or_ct.as_ref(),
                        aad: state.aad.as_ref(),
                    };
                    let mut ct = match cipher.encrypt(nonce, payload) {
                        Ok(ct) => ct,
                        Err(_) => return nanbox_undefined(),
                    };
                    let tag = ct.split_off(ct.len().saturating_sub(16));
                    state.auth_tag = Some(tag[..state.auth_tag_len.min(tag.len())].to_vec());
                    ct
                }
                (CipherKind::Aes192Gcm, true) => {
                    use aes_gcm::aead::{Aead, KeyInit, Payload};
                    use aes_gcm::Nonce;
                    let cipher = match Aes192Gcm::new_from_slice(&state.key) {
                        Ok(c) => c,
                        Err(_) => return nanbox_undefined(),
                    };
                    let nonce = Nonce::from_slice(&state.iv);
                    let payload = Payload {
                        msg: plaintext_or_ct.as_ref(),
                        aad: state.aad.as_ref(),
                    };
                    let mut ct = match cipher.encrypt(nonce, payload) {
                        Ok(ct) => ct,
                        Err(_) => return nanbox_undefined(),
                    };
                    let tag = ct.split_off(ct.len().saturating_sub(16));
                    state.auth_tag = Some(tag[..state.auth_tag_len.min(tag.len())].to_vec());
                    ct
                }
                (CipherKind::Aes256Gcm, false) => {
                    let tag = match state.auth_tag.as_ref() {
                        Some(t) if t.len() == state.auth_tag_len => t.clone(),
                        _ => return nanbox_undefined(), // GCM decrypt needs tag
                    };
                    match decrypt_gcm256_with_tag_len(
                        &state.key,
                        &state.iv,
                        &state.aad,
                        &plaintext_or_ct,
                        &tag,
                    ) {
                        Some(pt) => pt,
                        None => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes192Gcm, false) => {
                    let tag = match state.auth_tag.as_ref() {
                        Some(t) if t.len() == state.auth_tag_len => t.clone(),
                        _ => return nanbox_undefined(),
                    };
                    match decrypt_gcm192_with_tag_len(
                        &state.key,
                        &state.iv,
                        &state.aad,
                        &plaintext_or_ct,
                        &tag,
                    ) {
                        Some(pt) => pt,
                        None => return nanbox_undefined(),
                    }
                }
                (CipherKind::Aes128Gcm, false) => {
                    let tag = match state.auth_tag.as_ref() {
                        Some(t) if t.len() == state.auth_tag_len => t.clone(),
                        _ => return nanbox_undefined(),
                    };
                    match decrypt_gcm128_with_tag_len(
                        &state.key,
                        &state.iv,
                        &state.aad,
                        &plaintext_or_ct,
                        &tag,
                    ) {
                        Some(pt) => pt,
                        None => return nanbox_undefined(),
                    }
                }
            };
            let buf = alloc_buffer_from_slice(&output);
            nanbox_pointer_f64(buf as usize)
        }
        // `.getAuthTag()` — GCM-encrypt only. Returns the 16-byte tag
        // that `.final()` stashed. Calling this before `.final()` (or on
        // a non-GCM cipher) yields undefined.
        "getAuthTag" => match state.auth_tag.as_ref() {
            Some(tag) => {
                let buf = alloc_buffer_from_slice(tag);
                nanbox_pointer_f64(buf as usize)
            }
            None => nanbox_undefined(),
        },
        // `.setAuthTag(tag)` — GCM-decrypt only. Stores the tag so
        // `.final()` can authenticate. Returns the handle (Node returns
        // `this`); the chain-call surface in Perry doesn't rely on the
        // return shape, but mirroring Node's API matters for the rare
        // chained `d.setAuthTag(t).update(x).final()` case.
        "setAuthTag" => {
            if args.is_empty() {
                return nanbox_undefined();
            }
            let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            let tag = bytes_from_ptr(ptr);
            state.auth_tag = Some(tag);
            nanbox_pointer_f64(handle as usize)
        }
        // `.setAAD(buf)` — bind additional authenticated data for GCM.
        "setAAD" => {
            if args.is_empty() {
                state.aad.clear();
            } else {
                let ptr = (args[0].to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
                state.aad = bytes_from_ptr(ptr);
            }
            nanbox_pointer_f64(handle as usize)
        }
        // `.setAutoPadding([autoPadding])` — Node defaults to PKCS#7
        // padding for CBC/ECB and allows callers to disable it for exact
        // block-size inputs. Return `this` for chaining.
        "setAutoPadding" => {
            state.auto_padding = args.first().copied().map(js_truthy).unwrap_or(true);
            nanbox_pointer_f64(handle as usize)
        }
        _ => nanbox_undefined(),
    }
}

/// Property reads on a CipherHandle — `c.getAuthTag` / `c.setAuthTag` /
/// `c.update` / `c.final` / `c.setAAD`. Issue #1111: without this,
/// `c.getAuthTag?.()` short-circuited because the property access
/// returned undefined (small handles have no field storage), so the
/// `?.` lowering's `c.getAuthTag == null` check fired and the call
/// never happened.
///
/// Each known method name returns a bound-method closure (via
/// `js_class_method_bind`) whose `this` is the POINTER_TAG-NaN-boxed
/// handle. When invoked the closure routes through
/// `js_native_call_method` → `HANDLE_METHOD_DISPATCH` → `dispatch_cipher`,
/// the exact path `c.method(args)` takes when called inline. So
/// `typeof c.getAuthTag === "function"` and `const g = c.getAuthTag; g()`
/// both work, mirroring Node's `Cipher` shape.
pub unsafe fn dispatch_cipher_property(handle: i64, property: &str) -> f64 {
    let name_bytes: &'static [u8] = match property {
        "update" => b"update",
        "final" => b"final",
        "getAuthTag" => b"getAuthTag",
        "setAuthTag" => b"setAuthTag",
        "setAAD" => b"setAAD",
        "setAutoPadding" => b"setAutoPadding",
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
