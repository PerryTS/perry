use super::*;

#[derive(Clone, Copy)]
struct DerNode {
    tag: u8,
    header_len: usize,
    len: usize,
    start: usize,
}

impl DerNode {
    fn end(self) -> usize {
        self.start + self.len
    }

    fn full_range(self) -> std::ops::Range<usize> {
        (self.start - self.header_len)..self.end()
    }
}

struct SpkacParts<'a> {
    public_key_and_challenge_der: &'a [u8],
    spki_der: &'a [u8],
    challenge: &'a [u8],
    signature: &'a [u8],
}

const MD5_WITH_RSA_ENCRYPTION_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x04];

fn read_der_node(input: &[u8], offset: usize) -> Option<DerNode> {
    let tag = *input.get(offset)?;
    let len0 = *input.get(offset + 1)?;
    if len0 & 0x80 == 0 {
        let len = len0 as usize;
        let start = offset + 2;
        return (start + len <= input.len()).then_some(DerNode {
            tag,
            header_len: 2,
            len,
            start,
        });
    }

    let n = (len0 & 0x7f) as usize;
    if n == 0 || n > 4 || offset + 2 + n > input.len() {
        return None;
    }
    let mut len = 0usize;
    for b in &input[offset + 2..offset + 2 + n] {
        len = (len << 8) | (*b as usize);
    }
    let start = offset + 2 + n;
    (start + len <= input.len()).then_some(DerNode {
        tag,
        header_len: 2 + n,
        len,
        start,
    })
}

fn decode_spkac_input(bytes: Vec<u8>) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(&bytes).ok()?.trim();
    base64::engine::general_purpose::STANDARD
        .decode(text.as_bytes())
        .ok()
}

fn parse_spkac(bytes: &[u8]) -> Option<SpkacParts<'_>> {
    let outer = read_der_node(bytes, 0)?;
    if outer.tag != 0x30 || outer.end() != bytes.len() {
        return None;
    }

    let pkac = read_der_node(bytes, outer.start)?;
    if pkac.tag != 0x30 {
        return None;
    }
    let sig_alg = read_der_node(bytes, pkac.end())?;
    if sig_alg.tag != 0x30 {
        return None;
    }
    if !is_md5_rsa_signature_algorithm(bytes, sig_alg) {
        return None;
    }
    let sig = read_der_node(bytes, sig_alg.end())?;
    if sig.tag != 0x03 || sig.end() != outer.end() || sig.len == 0 {
        return None;
    }

    let spki = read_der_node(bytes, pkac.start)?;
    if spki.tag != 0x30 {
        return None;
    }
    let challenge = read_der_node(bytes, spki.end())?;
    if challenge.tag != 0x16 || challenge.end() != pkac.end() {
        return None;
    }
    if bytes[sig.start] != 0 {
        return None;
    }

    Some(SpkacParts {
        public_key_and_challenge_der: &bytes[pkac.full_range()],
        spki_der: &bytes[spki.full_range()],
        challenge: &bytes[challenge.start..challenge.end()],
        signature: &bytes[sig.start + 1..sig.end()],
    })
}

fn is_md5_rsa_signature_algorithm(bytes: &[u8], alg: DerNode) -> bool {
    let Some(oid) = read_der_node(bytes, alg.start) else {
        return false;
    };
    if oid.tag != 0x06 || &bytes[oid.start..oid.end()] != MD5_WITH_RSA_ENCRYPTION_OID {
        return false;
    }

    if oid.end() == alg.end() {
        return true;
    }

    let Some(params) = read_der_node(bytes, oid.end()) else {
        return false;
    };
    params.tag == 0x05 && params.len == 0 && params.end() == alg.end()
}

unsafe fn bytes_from_value(value: f64) -> Vec<u8> {
    if JSValue::from_bits(value.to_bits()).is_any_string() {
        bytes_from_ptr(perry_runtime::js_get_string_pointer_unified(value) as i64)
    } else {
        bytes_from_ptr(perry_runtime::js_nanbox_get_pointer(value))
    }
}

fn empty_buffer_value() -> f64 {
    let buf = unsafe { alloc_buffer_from_slice(&[]) };
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

fn pem_from_spki_der(spki_der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(spki_der);
    let mut pem = String::from("-----BEGIN PUBLIC KEY-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        pem.push('\n');
    }
    pem.push_str("-----END PUBLIC KEY-----\n");
    pem
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_certificate_verify_spkac(input: f64) -> f64 {
    let Some(der) = decode_spkac_input(bytes_from_value(input)) else {
        return js_bool(false);
    };
    let Some(parts) = parse_spkac(&der) else {
        return js_bool(false);
    };

    use rsa::pkcs8::DecodePublicKey;
    let public_key = match RsaPublicKey::from_public_key_der(parts.spki_der) {
        Ok(key) => key,
        Err(_) => return js_bool(false),
    };
    js_bool(verify_md5_rsa_pkcs1v15(
        &public_key,
        parts.public_key_and_challenge_der,
        parts.signature,
    ))
}

fn verify_md5_rsa_pkcs1v15(public_key: &RsaPublicKey, data: &[u8], signature: &[u8]) -> bool {
    let k = public_key.n().bits().div_ceil(8);
    if signature.len() != k {
        return false;
    }

    let sig = RsaBigUint::from_bytes_be(signature);
    let decoded = sig.modpow(public_key.e(), public_key.n()).to_bytes_be();
    if decoded.len() > k {
        return false;
    }
    let mut em = vec![0u8; k - decoded.len()];
    em.extend_from_slice(&decoded);

    if em.len() < 3 || em[0] != 0 || em[1] != 1 {
        return false;
    }
    let Some(sep) = em[2..].iter().position(|b| *b == 0).map(|i| i + 2) else {
        return false;
    };
    if sep < 10 || em[2..sep].iter().any(|b| *b != 0xff) {
        return false;
    }

    let mut md5 = Md5::new();
    Md5Digest::update(&mut md5, data);
    let digest = md5.finalize();
    const MD5_DIGEST_INFO_PREFIX: &[u8] = &[
        0x30, 0x20, 0x30, 0x0c, 0x06, 0x08, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x02, 0x05, 0x05,
        0x00, 0x04, 0x10,
    ];
    let digest_info = &em[sep + 1..];
    digest_info.len() == MD5_DIGEST_INFO_PREFIX.len() + digest.len()
        && digest_info.starts_with(MD5_DIGEST_INFO_PREFIX)
        && digest_info[MD5_DIGEST_INFO_PREFIX.len()..] == digest[..]
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_certificate_export_public_key(input: f64) -> f64 {
    let Some(der) = decode_spkac_input(bytes_from_value(input)) else {
        return empty_buffer_value();
    };
    let Some(parts) = parse_spkac(&der) else {
        return empty_buffer_value();
    };
    let pem = pem_from_spki_der(parts.spki_der);
    let buf = alloc_buffer_from_slice(pem.as_bytes());
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_certificate_export_challenge(input: f64) -> f64 {
    let Some(der) = decode_spkac_input(bytes_from_value(input)) else {
        return empty_buffer_value();
    };
    let Some(parts) = parse_spkac(&der) else {
        return empty_buffer_value();
    };
    let buf = alloc_buffer_from_slice(parts.challenge);
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}
