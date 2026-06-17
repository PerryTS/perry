use super::*;

// =====================================================================
// AES-GCM encrypt / decrypt
//
// jose's `gcmEncrypt` / `gcmDecrypt` pass:
//   { name: 'AES-GCM', iv: <Uint8Array>, additionalData?: <Uint8Array>,
//     tagLength?: 128 }, key, data
// The IV is a 12-byte nonce (the only length the underlying `aes-gcm`
// crate's `Nonce` type accepts); we surface a clean "undefined" reject
// for other lengths rather than panicking.
//
// The output of encrypt is `ciphertext || tag` (the WebCrypto spec
// appends the 16-byte GCM tag); decrypt expects the same layout.
// =====================================================================

/// Read an optional object field by name and return its raw bytes, or
/// `None` if the field is absent / not a buffer-like value.
pub(super) unsafe fn object_field_bytes(obj_bits: u64, name: &[u8]) -> Option<Vec<u8>> {
    let obj_ptr = strip_ptr(obj_bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    let bytes = bytes_from_jsvalue(val.bits());
    if bytes.is_empty() {
        // Distinguish "field missing" from "field present but empty":
        // for our callers an empty AAD / IV is semantically equivalent
        // to "missing", and the caller's defaulting path is fine.
        None
    } else {
        Some(bytes)
    }
}

pub(super) unsafe fn object_field_bits(obj_bits: u64, name: &[u8]) -> Option<u64> {
    let obj_ptr = strip_ptr(obj_bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    let bits = val.bits();
    if (bits >> 48) as u16 == 0x7FFC {
        None
    } else {
        Some(bits)
    }
}

/// Read an optional string field from an algorithm object.
pub(super) unsafe fn object_field_string(obj_bits: u64, name: &[u8]) -> Option<String> {
    let obj_ptr = strip_ptr(obj_bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    string_from_jsvalue(val.bits())
}

pub(super) unsafe fn set_object_string_field(
    obj: *mut perry_runtime::ObjectHeader,
    name: &[u8],
    value: &str,
) {
    let key = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = perry_runtime::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    js_object_set_field_by_name(
        obj,
        key,
        f64::from_bits(STRING_TAG | ((val as u64) & POINTER_MASK)),
    );
}

/// AES-GCM encrypt. Returns ciphertext || tag (matches WebCrypto spec).
pub(super) fn aes_gcm_encrypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce};
    type Aes192Gcm = aes_gcm::AesGcm<Aes192, ::aes::cipher::consts::U12>;

    if iv.len() != 12 {
        return None;
    }
    let nonce = Nonce::from_slice(iv);
    let payload = Payload {
        msg: plaintext,
        aad,
    };
    match key.len() {
        16 => {
            let cipher = Aes128Gcm::new_from_slice(key).ok()?;
            cipher.encrypt(nonce, payload).ok()
        }
        24 => {
            let cipher = Aes192Gcm::new_from_slice(key).ok()?;
            cipher.encrypt(nonce, payload).ok()
        }
        32 => {
            let cipher = Aes256Gcm::new_from_slice(key).ok()?;
            cipher.encrypt(nonce, payload).ok()
        }
        _ => None,
    }
}

/// AES-GCM decrypt. Expects `ciphertext || tag` per the WebCrypto spec.
pub(super) fn aes_gcm_decrypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce};
    type Aes192Gcm = aes_gcm::AesGcm<Aes192, ::aes::cipher::consts::U12>;

    if iv.len() != 12 {
        return None;
    }
    let nonce = Nonce::from_slice(iv);
    let payload = Payload {
        msg: ciphertext,
        aad,
    };
    match key.len() {
        16 => {
            let cipher = Aes128Gcm::new_from_slice(key).ok()?;
            cipher.decrypt(nonce, payload).ok()
        }
        24 => {
            let cipher = Aes192Gcm::new_from_slice(key).ok()?;
            cipher.decrypt(nonce, payload).ok()
        }
        32 => {
            let cipher = Aes256Gcm::new_from_slice(key).ok()?;
            cipher.decrypt(nonce, payload).ok()
        }
        _ => None,
    }
}

pub(super) fn chacha20_poly1305_encrypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Option<Vec<u8>> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{ChaCha20Poly1305, Nonce};

    if key.len() != 32 || iv.len() != 12 {
        return None;
    }
    let cipher = ChaCha20Poly1305::new_from_slice(key).ok()?;
    cipher
        .encrypt(
            Nonce::from_slice(iv),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .ok()
}

pub(super) fn chacha20_poly1305_decrypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Option<Vec<u8>> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{ChaCha20Poly1305, Nonce};

    if key.len() != 32 || iv.len() != 12 {
        return None;
    }
    let cipher = ChaCha20Poly1305::new_from_slice(key).ok()?;
    cipher
        .decrypt(
            Nonce::from_slice(iv),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .ok()
}

#[derive(Clone)]
struct AesOcbConstants {
    l_star: [u8; 16],
    l_dollar: [u8; 16],
    l: Vec<[u8; 16]>,
}

impl AesOcbConstants {
    fn new<C: BlockEncrypt>(cipher: &C) -> Self {
        let mut l_star = [0u8; 16];
        aes_ocb_encrypt_block(cipher, &mut l_star);
        let l_dollar = aes_ocb_double(&l_star);
        let l0 = aes_ocb_double(&l_dollar);
        Self {
            l_star,
            l_dollar,
            l: vec![l0],
        }
    }

    fn l(&mut self, index: usize) -> [u8; 16] {
        while self.l.len() <= index {
            let last = *self.l.last().expect("OCB L table is initialized");
            self.l.push(aes_ocb_double(&last));
        }
        self.l[index]
    }
}

fn aes_ocb_encrypt_block<C: BlockEncrypt>(cipher: &C, block: &mut [u8; 16]) {
    let mut block_arr = GenericArray::clone_from_slice(block);
    cipher.encrypt_block(&mut block_arr);
    block.copy_from_slice(&block_arr);
}

fn aes_ocb_decrypt_block<C: BlockDecrypt>(cipher: &C, block: &mut [u8; 16]) {
    let mut block_arr = GenericArray::clone_from_slice(block);
    cipher.decrypt_block(&mut block_arr);
    block.copy_from_slice(&block_arr);
}

fn aes_ocb_double(block: &[u8; 16]) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut carry = 0u8;
    for i in (0..16).rev() {
        let byte = block[i];
        out[i] = (byte << 1) | carry;
        carry = byte >> 7;
    }
    if block[0] & 0x80 != 0 {
        out[15] ^= 0x87;
    }
    out
}

fn aes_ocb_xor_block(dst: &mut [u8; 16], rhs: &[u8; 16]) {
    for (a, b) in dst.iter_mut().zip(rhs) {
        *a ^= *b;
    }
}

fn aes_ocb_ntz(n: usize) -> usize {
    n.trailing_zeros() as usize
}

pub(super) fn aes_ocb_tag_len_bytes(tag_len_bits: u32) -> Option<usize> {
    match tag_len_bits {
        64 => Some(8),
        96 => Some(12),
        128 => Some(16),
        _ => None,
    }
}

fn aes_ocb_initial_offset<C: BlockEncrypt>(
    cipher: &C,
    nonce: &[u8],
    tag_len_bytes: usize,
) -> Option<[u8; 16]> {
    if nonce.is_empty() || nonce.len() > 15 {
        return None;
    }

    let mut nonce_block = [0u8; 16];
    nonce_block[0] = ((((tag_len_bytes as u32 * 8) % 128) << 1) & 0xff) as u8;
    let start = 16 - nonce.len();
    nonce_block[start..].copy_from_slice(nonce);
    nonce_block[start - 1] |= 1;

    let bottom = (nonce_block[15] & 0x3f) as usize;
    let top = u128::from_be_bytes(nonce_block) & !0x3fu128;
    let mut ktop = top.to_be_bytes();
    aes_ocb_encrypt_block(cipher, &mut ktop);

    let mut stretch = [0u8; 24];
    stretch[..16].copy_from_slice(&ktop);
    for i in 0..8 {
        stretch[16 + i] = ktop[i] ^ ktop[i + 1];
    }

    let stretch_low = u128::from_be_bytes(stretch[..16].try_into().ok()?);
    let stretch_hi = u64::from_be_bytes(stretch[16..24].try_into().ok()?);
    let offset = (stretch_low << bottom) | (u128::from(stretch_hi) >> (64 - bottom));
    Some(offset.to_be_bytes())
}

fn aes_ocb_hash<C: BlockEncrypt>(
    cipher: &C,
    constants: &mut AesOcbConstants,
    associated_data: &[u8],
) -> [u8; 16] {
    let mut offset = [0u8; 16];
    let mut sum = [0u8; 16];
    let mut i = 1usize;
    let mut chunks = associated_data.chunks_exact(16);
    for chunk in &mut chunks {
        let l = constants.l(aes_ocb_ntz(i));
        aes_ocb_xor_block(&mut offset, &l);

        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        aes_ocb_xor_block(&mut block, &offset);
        aes_ocb_encrypt_block(cipher, &mut block);
        aes_ocb_xor_block(&mut sum, &block);
        i += 1;
    }

    let remaining = chunks.remainder();
    if !remaining.is_empty() {
        let l_star = constants.l_star;
        aes_ocb_xor_block(&mut offset, &l_star);
        let mut block = [0u8; 16];
        block[..remaining.len()].copy_from_slice(remaining);
        block[remaining.len()] = 0x80;
        aes_ocb_xor_block(&mut block, &offset);
        aes_ocb_encrypt_block(cipher, &mut block);
        aes_ocb_xor_block(&mut sum, &block);
    }

    sum
}

fn aes_ocb_compute_tag<C: BlockEncrypt>(
    cipher: &C,
    constants: &mut AesOcbConstants,
    aad: &[u8],
    checksum: &[u8; 16],
    offset: &[u8; 16],
) -> [u8; 16] {
    let mut tag = *checksum;
    aes_ocb_xor_block(&mut tag, offset);
    let l_dollar = constants.l_dollar;
    aes_ocb_xor_block(&mut tag, &l_dollar);
    aes_ocb_encrypt_block(cipher, &mut tag);
    let hash = aes_ocb_hash(cipher, constants, aad);
    aes_ocb_xor_block(&mut tag, &hash);
    tag
}

fn aes_ocb_encrypt_with_cipher<C: BlockEncrypt>(
    cipher: &C,
    iv: &[u8],
    aad: &[u8],
    plaintext: &[u8],
    tag_len_bytes: usize,
) -> Option<Vec<u8>> {
    let mut constants = AesOcbConstants::new(cipher);
    let mut offset = aes_ocb_initial_offset(cipher, iv, tag_len_bytes)?;
    let mut checksum = [0u8; 16];
    let mut out = Vec::with_capacity(plaintext.len() + tag_len_bytes);

    let mut i = 1usize;
    let mut chunks = plaintext.chunks_exact(16);
    for chunk in &mut chunks {
        let l = constants.l(aes_ocb_ntz(i));
        aes_ocb_xor_block(&mut offset, &l);

        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        aes_ocb_xor_block(&mut checksum, &block);
        aes_ocb_xor_block(&mut block, &offset);
        aes_ocb_encrypt_block(cipher, &mut block);
        aes_ocb_xor_block(&mut block, &offset);
        out.extend_from_slice(&block);
        i += 1;
    }

    let remaining = chunks.remainder();
    if !remaining.is_empty() {
        let l_star = constants.l_star;
        aes_ocb_xor_block(&mut offset, &l_star);
        let mut pad = offset;
        aes_ocb_encrypt_block(cipher, &mut pad);

        let mut padded = [0u8; 16];
        padded[..remaining.len()].copy_from_slice(remaining);
        padded[remaining.len()] = 0x80;
        aes_ocb_xor_block(&mut checksum, &padded);
        out.extend(remaining.iter().zip(pad.iter()).map(|(a, b)| a ^ b));
    }

    let tag = aes_ocb_compute_tag(cipher, &mut constants, aad, &checksum, &offset);
    out.extend_from_slice(&tag[..tag_len_bytes]);
    Some(out)
}

fn aes_ocb_decrypt_with_cipher<C: BlockDecrypt + BlockEncrypt>(
    cipher: &C,
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
    tag_len_bytes: usize,
) -> Option<Vec<u8>> {
    if ciphertext.len() < tag_len_bytes {
        return None;
    }
    let body_len = ciphertext.len() - tag_len_bytes;
    let (body, provided_tag) = ciphertext.split_at(body_len);
    let mut constants = AesOcbConstants::new(cipher);
    let mut offset = aes_ocb_initial_offset(cipher, iv, tag_len_bytes)?;
    let mut checksum = [0u8; 16];
    let mut out = Vec::with_capacity(body.len());

    let mut i = 1usize;
    let mut chunks = body.chunks_exact(16);
    for chunk in &mut chunks {
        let l = constants.l(aes_ocb_ntz(i));
        aes_ocb_xor_block(&mut offset, &l);

        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        aes_ocb_xor_block(&mut block, &offset);
        aes_ocb_decrypt_block(cipher, &mut block);
        aes_ocb_xor_block(&mut block, &offset);
        aes_ocb_xor_block(&mut checksum, &block);
        out.extend_from_slice(&block);
        i += 1;
    }

    let remaining = chunks.remainder();
    if !remaining.is_empty() {
        let l_star = constants.l_star;
        aes_ocb_xor_block(&mut offset, &l_star);
        let mut pad = offset;
        aes_ocb_encrypt_block(cipher, &mut pad);

        let start = out.len();
        out.extend(remaining.iter().zip(pad.iter()).map(|(a, b)| a ^ b));
        let partial = &out[start..];
        let mut padded = [0u8; 16];
        padded[..partial.len()].copy_from_slice(partial);
        padded[partial.len()] = 0x80;
        aes_ocb_xor_block(&mut checksum, &padded);
    }

    let expected_tag = aes_ocb_compute_tag(cipher, &mut constants, aad, &checksum, &offset);
    if !constant_time_eq(&expected_tag[..tag_len_bytes], provided_tag) {
        return None;
    }
    Some(out)
}

pub(super) fn aes_ocb_encrypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    plaintext: &[u8],
    tag_len_bits: u32,
) -> Option<Vec<u8>> {
    let tag_len_bytes = aes_ocb_tag_len_bytes(tag_len_bits)?;
    match key.len() {
        16 => {
            let cipher = Aes128::new_from_slice(key).ok()?;
            aes_ocb_encrypt_with_cipher(&cipher, iv, aad, plaintext, tag_len_bytes)
        }
        24 => {
            let cipher = Aes192::new_from_slice(key).ok()?;
            aes_ocb_encrypt_with_cipher(&cipher, iv, aad, plaintext, tag_len_bytes)
        }
        32 => {
            let cipher = Aes256::new_from_slice(key).ok()?;
            aes_ocb_encrypt_with_cipher(&cipher, iv, aad, plaintext, tag_len_bytes)
        }
        _ => None,
    }
}

pub(super) fn aes_ocb_decrypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
    tag_len_bits: u32,
) -> Option<Vec<u8>> {
    let tag_len_bytes = aes_ocb_tag_len_bytes(tag_len_bits)?;
    match key.len() {
        16 => {
            let cipher = Aes128::new_from_slice(key).ok()?;
            aes_ocb_decrypt_with_cipher(&cipher, iv, aad, ciphertext, tag_len_bytes)
        }
        24 => {
            let cipher = Aes192::new_from_slice(key).ok()?;
            aes_ocb_decrypt_with_cipher(&cipher, iv, aad, ciphertext, tag_len_bytes)
        }
        32 => {
            let cipher = Aes256::new_from_slice(key).ok()?;
            aes_ocb_decrypt_with_cipher(&cipher, iv, aad, ciphertext, tag_len_bytes)
        }
        _ => None,
    }
}

pub(super) type Aes128CbcEnc = Encryptor<Aes128>;
pub(super) type Aes192CbcEnc = Encryptor<Aes192>;
pub(super) type Aes256CbcEnc = Encryptor<Aes256>;
pub(super) type Aes128CbcDec = Decryptor<Aes128>;
pub(super) type Aes192CbcDec = Decryptor<Aes192>;
pub(super) type Aes256CbcDec = Decryptor<Aes256>;

pub(super) fn aes_cbc_encrypt(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Option<Vec<u8>> {
    if iv.len() != 16 {
        return None;
    }
    let padded_len = ((plaintext.len() / 16) + 1) * 16;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext.len()].copy_from_slice(plaintext);
    let out = match key.len() {
        16 => Aes128CbcEnc::new_from_slices(key, iv)
            .ok()?
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .ok()?,
        24 => Aes192CbcEnc::new_from_slices(key, iv)
            .ok()?
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .ok()?,
        32 => Aes256CbcEnc::new_from_slices(key, iv)
            .ok()?
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .ok()?,
        _ => return None,
    };
    Some(out.to_vec())
}

pub(super) fn aes_cbc_decrypt(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    if iv.len() != 16 || ciphertext.is_empty() || ciphertext.len() % 16 != 0 {
        return None;
    }
    let mut buf = ciphertext.to_vec();
    let out = match key.len() {
        16 => Aes128CbcDec::new_from_slices(key, iv)
            .ok()?
            .decrypt_padded_mut::<Pkcs7>(&mut buf)
            .ok()?,
        24 => Aes192CbcDec::new_from_slices(key, iv)
            .ok()?
            .decrypt_padded_mut::<Pkcs7>(&mut buf)
            .ok()?,
        32 => Aes256CbcDec::new_from_slices(key, iv)
            .ok()?
            .decrypt_padded_mut::<Pkcs7>(&mut buf)
            .ok()?,
        _ => return None,
    };
    Some(out.to_vec())
}

pub(super) unsafe fn extract_aes_cbc_args(
    algo_bits: u64,
    key_bits: u64,
    data_bits: u64,
) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let algo_name = extract_algo_name(algo_bits)?;
    if !algo_name.eq_ignore_ascii_case("AES-CBC") {
        return None;
    }
    let iv = object_field_bytes(algo_bits, b"iv")?;
    let key_addr = strip_ptr(key_bits);
    let mat = lookup_crypto_key(key_addr)?;
    if mat.algo != KeyAlgo::AesCbc {
        return None;
    }
    let key_bytes = bytes_from_jsvalue(key_bits);
    let data_bytes = bytes_from_jsvalue(data_bits);
    Some((key_bytes, iv, data_bytes))
}

/// Shared AES-GCM arg-extraction for encrypt / decrypt: pulls the
/// algorithm-name + iv (+ optional aad) from the algorithm object, plus
/// the raw key bytes (validating they came from an AES-GCM importKey)
/// and the data bytes. Returns `None` if any required piece is missing.

pub(super) fn increment_ctr_counter(counter: &mut [u8; 16], length: u32) {
    let n = u128::from_be_bytes(*counter);
    let mask = if length == 128 {
        u128::MAX
    } else {
        (1u128 << length) - 1
    };
    let prefix = n & !mask;
    let next = ((n & mask).wrapping_add(1)) & mask;
    *counter = (prefix | next).to_be_bytes();
}

pub(super) fn aes_ctr_apply(
    key: &[u8],
    counter: &[u8],
    length: u32,
    data: &[u8],
) -> Option<Vec<u8>> {
    if counter.len() != 16 || length == 0 || length > 128 {
        return None;
    }
    let mut ctr = [0u8; 16];
    ctr.copy_from_slice(counter);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut block = GenericArray::clone_from_slice(&ctr);
        match key.len() {
            16 => Aes128::new_from_slice(key).ok()?.encrypt_block(&mut block),
            24 => Aes192::new_from_slice(key).ok()?.encrypt_block(&mut block),
            32 => Aes256::new_from_slice(key).ok()?.encrypt_block(&mut block),
            _ => return None,
        }
        out.extend(chunk.iter().zip(block.iter()).map(|(a, b)| a ^ b));
        increment_ctr_counter(&mut ctr, length);
    }
    Some(out)
}

pub(super) unsafe fn extract_aes_ctr_args(
    algo_bits: u64,
    key_bits: u64,
    data_bits: u64,
) -> Option<(Vec<u8>, Vec<u8>, u32, Vec<u8>)> {
    let algo_name = extract_algo_name(algo_bits)?;
    if !algo_name.eq_ignore_ascii_case("AES-CTR") {
        return None;
    }
    let counter = object_field_bytes(algo_bits, b"counter")?;
    let length = object_field_number(algo_bits, b"length")?;
    let key_addr = strip_ptr(key_bits);
    let mat = lookup_crypto_key(key_addr)?;
    if mat.algo != KeyAlgo::AesCtr {
        return None;
    }
    let key_bytes = bytes_from_jsvalue(key_bits);
    let data_bytes = bytes_from_jsvalue(data_bits);
    Some((key_bytes, counter, length, data_bytes))
}

pub(super) unsafe fn extract_aes_gcm_args(
    algo_bits: u64,
    key_bits: u64,
    data_bits: u64,
) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> {
    let algo_name = extract_algo_name(algo_bits)?;
    if !algo_name.eq_ignore_ascii_case("AES-GCM") {
        return None;
    }
    let iv = object_field_bytes(algo_bits, b"iv")?;
    let aad = object_field_bytes(algo_bits, b"additionalData").unwrap_or_default();
    let key_addr = strip_ptr(key_bits);
    let mat = lookup_crypto_key(key_addr)?;
    if mat.algo != KeyAlgo::AesGcm {
        return None;
    }
    let key_bytes = bytes_from_jsvalue(key_bits);
    let data_bytes = bytes_from_jsvalue(data_bits);
    Some((key_bytes, iv, aad, data_bytes))
}

pub(super) unsafe fn extract_chacha20_poly1305_args(
    algo_bits: u64,
    key_bits: u64,
    data_bits: u64,
) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> {
    let algo_name = extract_algo_name(algo_bits)?;
    if !algo_name.eq_ignore_ascii_case("ChaCha20-Poly1305") {
        return None;
    }
    let iv = object_field_bytes(algo_bits, b"iv")?;
    if let Some(tag_length) = object_field_number(algo_bits, b"tagLength") {
        if tag_length != 128 {
            return None;
        }
    }
    let aad = object_field_bytes(algo_bits, b"additionalData").unwrap_or_default();
    let key_addr = strip_ptr(key_bits);
    let mat = lookup_crypto_key(key_addr)?;
    if mat.algo != KeyAlgo::ChaCha20Poly1305 {
        return None;
    }
    let key_bytes = bytes_from_jsvalue(key_bits);
    let data_bytes = bytes_from_jsvalue(data_bits);
    Some((key_bytes, iv, aad, data_bytes))
}

pub(super) unsafe fn extract_aes_ocb_args(
    algo_bits: u64,
    key_bits: u64,
    data_bits: u64,
) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>, u32, Vec<u8>)> {
    let algo_name = extract_algo_name(algo_bits)?;
    if !algo_name.eq_ignore_ascii_case("AES-OCB") {
        return None;
    }
    let iv = object_field_bytes(algo_bits, b"iv")?;
    let aad = object_field_bytes(algo_bits, b"additionalData").unwrap_or_default();
    let tag_len = object_field_number(algo_bits, b"tagLength").unwrap_or(128);
    aes_ocb_tag_len_bytes(tag_len)?;
    let key_addr = strip_ptr(key_bits);
    let mat = lookup_crypto_key(key_addr)?;
    if mat.algo != KeyAlgo::AesOcb {
        return None;
    }
    let key_bytes = bytes_from_jsvalue(key_bits);
    let data_bytes = bytes_from_jsvalue(data_bits);
    Some((key_bytes, iv, aad, tag_len, data_bytes))
}

/// `crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData? }, key, data)`
/// → Promise<Uint8Array>
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_encrypt(
    algo_bits: f64,
    key_bits: f64,
    data_bits: f64,
) -> *mut Promise {
    let algo_name = match extract_algo_name(algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    if algo_name.eq_ignore_ascii_case("RSA-OAEP") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::RsaOaep || mat.kind != KeyKind::Public {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        if let Err((name, message)) = require_usage(
            mat,
            USAGE_ENCRYPT,
            "The requested operation is not valid for the provided key",
        ) {
            return reject_with_dom_exception(name, message);
        }
        let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
        let public_key = match RsaPublicKey::from_public_key_der(&key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let data = bytes_from_jsvalue(data_bits.to_bits());
        let ciphertext = match rsa_oaep_encrypt(mat.hash, &public_key, &data) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&ciphertext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CBC") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::AesCbc {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        if let Err((name, message)) = require_usage(
            mat,
            USAGE_ENCRYPT,
            "The requested operation is not valid for the provided key",
        ) {
            return reject_with_dom_exception(name, message);
        }
        let (key, iv, data) = match extract_aes_cbc_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let ciphertext = match aes_cbc_encrypt(&key, &iv, &data) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&ciphertext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CTR") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::AesCtr {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        if let Err((name, message)) = require_usage(
            mat,
            USAGE_ENCRYPT,
            "The requested operation is not valid for the provided key",
        ) {
            return reject_with_dom_exception(name, message);
        }
        let (key, counter, length, data) = match extract_aes_ctr_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let ciphertext = match aes_ctr_apply(&key, &counter, length, &data) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&ciphertext);
    }
    if algo_name.eq_ignore_ascii_case("ChaCha20-Poly1305") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::ChaCha20Poly1305 {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        if let Err((name, message)) = require_usage(
            mat,
            USAGE_ENCRYPT,
            "The requested operation is not valid for the provided key",
        ) {
            return reject_with_dom_exception(name, message);
        }
        let (key, iv, aad, data) = match extract_chacha20_poly1305_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let ciphertext = match chacha20_poly1305_encrypt(&key, &iv, &aad, &data) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&ciphertext);
    }
    if algo_name.eq_ignore_ascii_case("AES-OCB") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::AesOcb {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        if let Err((name, message)) = require_usage(
            mat,
            USAGE_ENCRYPT,
            "The requested operation is not valid for the provided key",
        ) {
            return reject_with_dom_exception(name, message);
        }
        let (key, iv, aad, tag_len, data) = match extract_aes_ocb_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let ciphertext = match aes_ocb_encrypt(&key, &iv, &aad, &data, tag_len) {
            Some(c) => c,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&ciphertext);
    }
    if !algo_name.eq_ignore_ascii_case("AES-GCM") {
        return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name");
    }
    let key_addr = strip_ptr(key_bits.to_bits());
    let mat = match lookup_crypto_key(key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey")
        }
    };
    if mat.algo != KeyAlgo::AesGcm {
        return reject_with_dom_exception(
            "InvalidAccessError",
            "The requested operation is not valid for the provided key",
        );
    }
    if let Err((name, message)) = require_usage(
        mat,
        USAGE_ENCRYPT,
        "The requested operation is not valid for the provided key",
    ) {
        return reject_with_dom_exception(name, message);
    }
    let (key, iv, aad, data) =
        match extract_aes_gcm_args(algo_bits.to_bits(), key_bits.to_bits(), data_bits.to_bits()) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
    let ciphertext = match aes_gcm_encrypt(&key, &iv, &aad, &data) {
        Some(c) => c,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    resolve_with_bytes(&ciphertext)
}

/// `crypto.subtle.decrypt({ name: "AES-GCM", iv, additionalData? }, key, data)`
/// → Promise<Uint8Array>
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_decrypt(
    algo_bits: f64,
    key_bits: f64,
    data_bits: f64,
) -> *mut Promise {
    let algo_name = match extract_algo_name(algo_bits.to_bits()) {
        Some(s) => s,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    if algo_name.eq_ignore_ascii_case("RSA-OAEP") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::RsaOaep || mat.kind != KeyKind::Private {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        if let Err((name, message)) = require_usage(
            mat,
            USAGE_DECRYPT,
            "The requested operation is not valid for the provided key",
        ) {
            return reject_with_dom_exception(name, message);
        }
        let key_bytes = bytes_from_jsvalue(key_bits.to_bits());
        let private_key = match RsaPrivateKey::from_pkcs8_der(&key_bytes) {
            Ok(k) => k,
            Err(_) => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let data = bytes_from_jsvalue(data_bits.to_bits());
        let plaintext = match rsa_oaep_decrypt(mat.hash, &private_key, &data) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&plaintext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CBC") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::AesCbc {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        if let Err((name, message)) = require_usage(
            mat,
            USAGE_DECRYPT,
            "The requested operation is not valid for the provided key",
        ) {
            return reject_with_dom_exception(name, message);
        }
        let (key, iv, data) = match extract_aes_cbc_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let plaintext = match aes_cbc_decrypt(&key, &iv, &data) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&plaintext);
    }
    if algo_name.eq_ignore_ascii_case("AES-CTR") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::AesCtr {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        if let Err((name, message)) = require_usage(
            mat,
            USAGE_DECRYPT,
            "The requested operation is not valid for the provided key",
        ) {
            return reject_with_dom_exception(name, message);
        }
        let (key, counter, length, data) = match extract_aes_ctr_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let plaintext = match aes_ctr_apply(&key, &counter, length, &data) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&plaintext);
    }
    if algo_name.eq_ignore_ascii_case("ChaCha20-Poly1305") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::ChaCha20Poly1305 {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        if let Err((name, message)) = require_usage(
            mat,
            USAGE_DECRYPT,
            "The requested operation is not valid for the provided key",
        ) {
            return reject_with_dom_exception(name, message);
        }
        let (key, iv, aad, data) = match extract_chacha20_poly1305_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let plaintext = match chacha20_poly1305_decrypt(&key, &iv, &aad, &data) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&plaintext);
    }
    if algo_name.eq_ignore_ascii_case("AES-OCB") {
        let key_addr = strip_ptr(key_bits.to_bits());
        let mat = match lookup_crypto_key(key_addr) {
            Some(m) => m,
            None => {
                return reject_with_dom_exception(
                    "InvalidAccessError",
                    "Key is not a valid CryptoKey",
                )
            }
        };
        if mat.algo != KeyAlgo::AesOcb {
            return reject_with_dom_exception(
                "InvalidAccessError",
                "The requested operation is not valid for the provided key",
            );
        }
        if let Err((name, message)) = require_usage(
            mat,
            USAGE_DECRYPT,
            "The requested operation is not valid for the provided key",
        ) {
            return reject_with_dom_exception(name, message);
        }
        let (key, iv, aad, tag_len, data) = match extract_aes_ocb_args(
            algo_bits.to_bits(),
            key_bits.to_bits(),
            data_bits.to_bits(),
        ) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        let plaintext = match aes_ocb_decrypt(&key, &iv, &aad, &data, tag_len) {
            Some(p) => p,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
        return resolve_with_bytes(&plaintext);
    }
    if !algo_name.eq_ignore_ascii_case("AES-GCM") {
        return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name");
    }
    let key_addr = strip_ptr(key_bits.to_bits());
    let mat = match lookup_crypto_key(key_addr) {
        Some(m) => m,
        None => {
            return reject_with_dom_exception("InvalidAccessError", "Key is not a valid CryptoKey")
        }
    };
    if mat.algo != KeyAlgo::AesGcm {
        return reject_with_dom_exception(
            "InvalidAccessError",
            "The requested operation is not valid for the provided key",
        );
    }
    if let Err((name, message)) = require_usage(
        mat,
        USAGE_DECRYPT,
        "The requested operation is not valid for the provided key",
    ) {
        return reject_with_dom_exception(name, message);
    }
    let (key, iv, aad, data) =
        match extract_aes_gcm_args(algo_bits.to_bits(), key_bits.to_bits(), data_bits.to_bits()) {
            Some(t) => t,
            None => return reject_with_dom_exception("OperationError", "The operation failed"),
        };
    let plaintext = match aes_gcm_decrypt(&key, &iv, &aad, &data) {
        Some(p) => p,
        None => return reject_with_dom_exception("OperationError", "The operation failed"),
    };
    resolve_with_bytes(&plaintext)
}

/// Read a numeric field from an algorithm object (`{ name, length }`).
/// Returns `None` if the field is absent or not a number. Required by
/// `generateKey({ name: 'AES-GCM', length: 256 }, ...)` — the spec
/// allows 128, 192, or 256 here but we only honor 128 and 256 (the
/// `aes-gcm` 0.10 crate doesn't ship a 192-bit type, matching the
/// existing encrypt/decrypt rejection at line ~547).
pub(super) unsafe fn object_field_number(obj_bits: u64, name: &[u8]) -> Option<u32> {
    let obj_ptr = strip_ptr(obj_bits) as *const perry_runtime::ObjectHeader;
    if (obj_ptr as usize) < 0x1000 {
        return None;
    }
    let key_ptr = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key_ptr);
    let bits = val.bits();
    let top16 = (bits >> 48) as u16;
    if top16 == 0x7FFE {
        // INT32_TAG — lower 32 bits as a signed int.
        let raw = (bits & 0xFFFF_FFFF) as i32;
        if raw >= 0 {
            return Some(raw as u32);
        }
        return None;
    }
    // Treat as f64. NaN-boxed primitives (undef, null) have non-finite
    // bits — reject them explicitly so callers fall back to the default.
    let f = f64::from_bits(bits);
    if f.is_finite() && f >= 0.0 && f <= u32::MAX as f64 {
        Some(f as u32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_128() -> [u8; 16] {
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
    }

    #[test]
    fn aes_ocb_node_vectors_by_iv_length() {
        let key = key_128();
        let data = b"hello ocb";
        let cases = [
            (1usize, "61c6b7fdd965807df4889d283f9536ec39b4a44eb17d7b8d15"),
            (5, "7f1e6ef775e81fc322648ea9dd0f3a85a8c3257fa20f21c224"),
            (6, "eaf843be631e91cb96242404046c46f4497ca6ae82798d0f65"),
            (12, "fd9dbb47148a60eb77fb39037f561406d4e133a17a7c6e9b9d"),
            (15, "1e0b9ba49f5656cb6051fafa79cc6c4e6cd401e32125fff39d"),
        ];

        for (iv_len, expected_hex) in cases {
            let iv = vec![7u8; iv_len];
            let ciphertext = aes_ocb_encrypt(&key, &iv, b"", data, 128).expect("encrypt");
            assert_eq!(hex::encode(&ciphertext), expected_hex);
            let plaintext = aes_ocb_decrypt(&key, &iv, b"", &ciphertext, 128).expect("decrypt");
            assert_eq!(plaintext, data);
        }
    }

    #[test]
    fn aes_ocb_node_vectors_for_aad_and_tag_lengths() {
        let key = key_128();
        let iv = [7u8; 12];
        let data = b"hello ocb";

        let with_aad = aes_ocb_encrypt(&key, &iv, &[1, 2], data, 128).expect("encrypt aad");
        assert_eq!(
            hex::encode(&with_aad),
            "fd9dbb47148a60eb77f5af17053aeea9ada06ecb00c0d8e018"
        );
        assert_eq!(
            aes_ocb_decrypt(&key, &iv, &[1, 2], &with_aad, 128).expect("decrypt aad"),
            data
        );
        assert!(aes_ocb_decrypt(&key, &iv, &[2, 1], &with_aad, 128).is_none());

        let tag96 = aes_ocb_encrypt(&key, &iv, b"", data, 96).expect("encrypt tag96");
        assert_eq!(
            hex::encode(&tag96),
            "3ed65543631f8ae93b3d2efa6871666366ee5295e6"
        );
        assert_eq!(
            aes_ocb_decrypt(&key, &iv, b"", &tag96, 96).expect("decrypt tag96"),
            data
        );

        let tag64 = aes_ocb_encrypt(&key, &iv, b"", data, 64).expect("encrypt tag64");
        assert_eq!(hex::encode(&tag64), "8994996d8cc184be7f831a45247b2ce8a7");
        assert_eq!(
            aes_ocb_decrypt(&key, &iv, b"", &tag64, 64).expect("decrypt tag64"),
            data
        );
    }

    #[test]
    fn aes_ocb_rejects_invalid_lengths_and_tampering() {
        let key = key_128();
        let iv = [7u8; 12];
        let data = b"hello ocb";
        assert!(aes_ocb_encrypt(&key, &[], b"", data, 128).is_none());
        assert!(aes_ocb_encrypt(&key, &[7u8; 16], b"", data, 128).is_none());
        assert!(aes_ocb_encrypt(&key, &iv, b"", data, 120).is_none());

        let mut ciphertext = aes_ocb_encrypt(&key, &iv, b"", data, 128).expect("encrypt");
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 1;
        assert!(aes_ocb_decrypt(&key, &iv, b"", &ciphertext, 128).is_none());
    }
}
