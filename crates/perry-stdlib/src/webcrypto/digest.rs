use super::*;

/// `crypto.subtle.digest(algorithm, data)` → Promise<Uint8Array>
///
/// `algorithm` is "SHA-1" / "SHA-256" / "SHA-384" / "SHA-512" (string)
/// or `{ name: "SHA-256" }`. Unknown algorithms reject with a TypeError.
#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_digest(algo_bits: f64, data_bits: f64) -> *mut Promise {
    let algo = match extract_hash_algo(algo_bits.to_bits()) {
        Some(a) => a,
        None => {
            return reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
        }
    };
    let bytes = bytes_from_jsvalue(data_bits.to_bits());
    let digest = compute_digest(algo, &bytes);
    resolve_with_bytes(&digest)
}
