//! One SHA-512 state shape for SHA-384/512 and their HMACs.
//!
//! OpenSSL dispatches compression by CPU features (including AVX2/BMI2 on
//! x86-64). Its fixed context owns the digest length, so copying a partial
//! hash preserves both its algorithm and buffered bytes. No EVP allocation
//! or application-level backend selection is needed.

use openssl_sys::{SHA384_Init, SHA512_Final, SHA512_Init, SHA512_Update, SHA512_CTX};

#[derive(Clone, Copy)]
pub enum Algorithm {
    Sha384,
    Sha512,
}

#[derive(Clone)]
pub struct Context(SHA512_CTX);

pub struct Output {
    bytes: [u8; 64],
    len: usize,
}

impl AsRef<[u8]> for Output {
    fn as_ref(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl Context {
    pub fn new(algorithm: Algorithm) -> Self {
        // SHA*_Init initializes the words, counters and digest length, but
        // need not initialize the unused block buffer. Zero it first so a
        // clone never reads uninitialized Rust integer fields.
        let mut context: SHA512_CTX = unsafe { std::mem::zeroed() };
        let initialized = unsafe {
            match algorithm {
                Algorithm::Sha384 => SHA384_Init(&mut context),
                Algorithm::Sha512 => SHA512_Init(&mut context),
            }
        };
        assert_eq!(initialized, 1);
        Self(context)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        // The context is initialized, the borrowed input lives through the
        // call, and OpenSSL retains no input pointer.
        let updated = unsafe { SHA512_Update(&mut self.0, bytes.as_ptr().cast(), bytes.len()) };
        assert_eq!(updated, 1);
    }

    pub fn finish(mut self) -> Output {
        let len = self.0.md_len as usize;
        let mut bytes = [0; 64];
        // SHA512_Final also handles SHA-384 using the context's md_len.
        let finalized = unsafe { SHA512_Final(bytes.as_mut_ptr(), &mut self.0) };
        assert_eq!(finalized, 1);
        Output { bytes, len }
    }
}

/// RFC 2104's two padded digest states, using the same context as Hash.
pub struct HmacContext {
    inner: Context,
    outer: Context,
}

impl HmacContext {
    pub fn new(algorithm: Algorithm, key: &[u8]) -> Self {
        let mut block = [0; 128];
        if key.len() > block.len() {
            let mut hash = Context::new(algorithm);
            hash.update(key);
            let key = hash.finish();
            block[..key.as_ref().len()].copy_from_slice(key.as_ref());
        } else {
            block[..key.len()].copy_from_slice(key);
        }
        for byte in &mut block {
            *byte ^= 0x36;
        }
        let mut inner = Context::new(algorithm);
        inner.update(&block);
        for byte in &mut block {
            *byte ^= 0x36 ^ 0x5c;
        }
        let mut outer = Context::new(algorithm);
        outer.update(&block);
        Self { inner, outer }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.inner.update(bytes);
    }

    pub fn sign(mut self) -> Output {
        self.outer.update(self.inner.finish().as_ref());
        self.outer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{KeyInit, Mac};
    use sha2::{Digest, Sha384, Sha512};

    #[test]
    fn wide_digest_preserves_algorithm_and_partial_block_when_cloned() {
        for len in [0, 1, 111, 112, 127, 128, 129, 255, 256, 513, 4097] {
            let data: Vec<_> = (0..len).map(|i| (i * 37) as u8).collect();
            for algorithm in [Algorithm::Sha384, Algorithm::Sha512] {
                let expected = match algorithm {
                    Algorithm::Sha384 => Sha384::digest(&data).to_vec(),
                    Algorithm::Sha512 => Sha512::digest(&data).to_vec(),
                };
                for chunk in [1, 17, 127, 128, 129, 4096] {
                    let mut hash = Context::new(algorithm);
                    let split = data.len().min(113);
                    hash.update(&data[..split]);
                    let mut copy = hash.clone();
                    for part in data[split..].chunks(chunk) {
                        hash.update(part);
                        copy.update(part);
                    }
                    assert_eq!(hash.finish().as_ref(), expected);
                    assert_eq!(copy.finish().as_ref(), expected);
                }
            }
        }
    }

    #[test]
    fn wide_hmac_matches_independent_backend_across_key_block_boundary() {
        for key_len in [0, 1, 127, 128, 129, 257] {
            let key: Vec<_> = (0..key_len).map(|i| (i * 19) as u8).collect();
            let data: Vec<_> = (0..4097).map(|i| (i * 37) as u8).collect();
            for algorithm in [Algorithm::Sha384, Algorithm::Sha512] {
                let expected = match algorithm {
                    Algorithm::Sha384 => {
                        let mut mac = hmac::Hmac::<Sha384>::new_from_slice(&key).unwrap();
                        mac.update(&data);
                        mac.finalize().into_bytes().to_vec()
                    }
                    Algorithm::Sha512 => {
                        let mut mac = hmac::Hmac::<Sha512>::new_from_slice(&key).unwrap();
                        mac.update(&data);
                        mac.finalize().into_bytes().to_vec()
                    }
                };
                let mut mac = HmacContext::new(algorithm, &key);
                for part in data.chunks(127) {
                    mac.update(part);
                }
                assert_eq!(mac.sign().as_ref(), expected);
            }
        }
    }
}
