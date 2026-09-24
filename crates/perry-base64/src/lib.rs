//! The base64 surface Perry uses: standard/URL alphabets, strict padded or
//! unpadded decoding, and optional padding for source maps. No JS allocation.
//! Node Buffer's permissive codecs remain in perry-runtime.
use std::fmt;

pub mod alphabet {
    pub struct Alphabet(pub(crate) &'static [u8; 64]);
    pub const STANDARD: Alphabet =
        Alphabet(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/");
    pub const URL_SAFE: Alphabet =
        Alphabet(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_");
}
pub mod engine {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum DecodePaddingMode {
        RequireCanonical,
        RequireNone,
        Indifferent,
    }
    pub mod general_purpose {
        use super::DecodePaddingMode;
        use crate::alphabet;
        #[derive(Clone, Copy)]
        pub struct GeneralPurposeConfig {
            pub(crate) padding: DecodePaddingMode,
        }
        impl Default for GeneralPurposeConfig {
            fn default() -> Self {
                Self::new()
            }
        }
        impl GeneralPurposeConfig {
            pub const fn new() -> Self {
                Self {
                    padding: DecodePaddingMode::RequireCanonical,
                }
            }
            pub const fn with_decode_padding_mode(mut self, mode: DecodePaddingMode) -> Self {
                self.padding = mode;
                self
            }
        }
        #[derive(Clone, Copy)]
        pub struct GeneralPurpose {
            pub(crate) alphabet: &'static [u8; 64],
            pub(crate) config: GeneralPurposeConfig,
            pub(crate) pad: bool,
        }
        impl GeneralPurpose {
            pub const fn new(alphabet: &alphabet::Alphabet, config: GeneralPurposeConfig) -> Self {
                Self {
                    alphabet: alphabet.0,
                    config,
                    pad: true,
                }
            }
        }
        pub const STANDARD: GeneralPurpose =
            GeneralPurpose::new(&alphabet::STANDARD, GeneralPurposeConfig::new());
        pub const STANDARD_NO_PAD: GeneralPurpose = GeneralPurpose {
            pad: false,
            config: GeneralPurposeConfig {
                padding: DecodePaddingMode::RequireNone,
            },
            ..STANDARD
        };
        pub const URL_SAFE: GeneralPurpose =
            GeneralPurpose::new(&alphabet::URL_SAFE, GeneralPurposeConfig::new());
        pub const URL_SAFE_NO_PAD: GeneralPurpose = GeneralPurpose {
            pad: false,
            config: GeneralPurposeConfig {
                padding: DecodePaddingMode::RequireNone,
            },
            ..URL_SAFE
        };
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    InvalidByte(usize, u8),
    InvalidLength(usize),
    InvalidLastSymbol(usize, u8),
    InvalidPadding,
}
impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::InvalidByte(i, b) => write!(f, "Invalid symbol {b}, offset {i}."),
            Self::InvalidLength(n) => write!(
                f,
                "Encoded text cannot have a 6-bit remainder (length {n})."
            ),
            Self::InvalidLastSymbol(i, b) => write!(f, "Invalid last symbol {b}, offset {i}."),
            Self::InvalidPadding => f.write_str("Invalid padding"),
        }
    }
}
impl std::error::Error for DecodeError {}

pub trait Engine {
    fn encode(&self, input: impl AsRef<[u8]>) -> String;
    fn decode(&self, input: impl AsRef<[u8]>) -> Result<Vec<u8>, DecodeError>;
}
impl Engine for engine::general_purpose::GeneralPurpose {
    fn encode(&self, input: impl AsRef<[u8]>) -> String {
        let input = input.as_ref();
        let mut out = String::with_capacity(input.len().div_ceil(3).saturating_mul(4));
        for chunk in input.chunks(3) {
            let a = chunk[0];
            let b = chunk.get(1).copied().unwrap_or(0);
            let c = chunk.get(2).copied().unwrap_or(0);
            out.push(char::from(self.alphabet[(a >> 2) as usize]));
            out.push(char::from(self.alphabet[((a & 3) << 4 | b >> 4) as usize]));
            if chunk.len() > 1 {
                out.push(char::from(self.alphabet[((b & 15) << 2 | c >> 6) as usize]));
            } else if self.pad {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(char::from(self.alphabet[(c & 63) as usize]));
            } else if self.pad {
                out.push('=');
            }
        }
        out
    }
    fn decode(&self, input: impl AsRef<[u8]>) -> Result<Vec<u8>, DecodeError> {
        use engine::DecodePaddingMode::*;
        let input = input.as_ref();
        let end = input.iter().position(|&b| b == b'=').unwrap_or(input.len());
        let padding = input.len() - end;
        if input[end..].iter().any(|&b| b != b'=') || padding > 2 {
            return Err(DecodeError::InvalidPadding);
        }
        if end % 4 == 1 {
            return Err(DecodeError::InvalidLength(end));
        }
        let required = (4 - end % 4) % 4;
        match self.config.padding {
            RequireCanonical if padding != required => return Err(DecodeError::InvalidPadding),
            RequireNone if padding != 0 => return Err(DecodeError::InvalidPadding),
            Indifferent if padding > required => return Err(DecodeError::InvalidPadding),
            _ => {}
        }
        let mut out = Vec::with_capacity(end / 4 * 3 + 2);
        let mut bits = 0u32;
        let mut count = 0;
        for (i, &b) in input[..end].iter().enumerate() {
            let value = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b if b == self.alphabet[62] => 62,
                b if b == self.alphabet[63] => 63,
                _ => return Err(DecodeError::InvalidByte(i, b)),
            };
            bits = (bits << 6) | u32::from(value);
            count += 6;
            if count >= 8 {
                count -= 8;
                out.push((bits >> count) as u8);
            }
        }
        if count > 0 && bits & ((1 << count) - 1) != 0 {
            return Err(DecodeError::InvalidLastSymbol(end - 1, input[end - 1]));
        }
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::{engine::general_purpose::*, *};
    #[test]
    fn rfc4648_vectors() {
        for (raw, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(STANDARD.encode(raw), encoded);
            assert_eq!(STANDARD.decode(encoded).unwrap(), raw.as_bytes());
            assert_eq!(
                STANDARD_NO_PAD
                    .decode(encoded.trim_end_matches('='))
                    .unwrap(),
                raw.as_bytes()
            );
        }
        assert_eq!(URL_SAFE_NO_PAD.encode([251, 255]), "-_8");
        assert_eq!(URL_SAFE_NO_PAD.decode("-_8").unwrap(), [251, 255]);
    }
    #[test]
    fn reject_noncanonical_security_inputs() {
        for text in [
            "Zg", "Zg=", "Zg===", "Zg==\n", "Zh==", "Zm9=", "=", "====", "Zm=v", "Z", "-_8=",
        ] {
            assert!(STANDARD.decode(text).is_err(), "{text}");
        }
        assert!(STANDARD_NO_PAD.decode("Zg==").is_err());
        assert!(URL_SAFE_NO_PAD.decode("+/8").is_err());
        let optional = GeneralPurpose::new(
            &alphabet::STANDARD,
            GeneralPurposeConfig::new()
                .with_decode_padding_mode(engine::DecodePaddingMode::Indifferent),
        );
        assert_eq!(optional.decode("Zg").unwrap(), b"f");
        assert_eq!(optional.decode("Zg==").unwrap(), b"f");
    }
}

#[cfg(test)]
mod compatibility {
    use super::{engine::general_purpose as ours, Engine};
    use base64::{engine::general_purpose as reference, Engine as _};
    #[test]
    fn codecs_match_reference_on_payloads_and_malformed_inputs() {
        let ours_optional = ours::GeneralPurpose::new(
            &super::alphabet::STANDARD,
            ours::GeneralPurposeConfig::new()
                .with_decode_padding_mode(super::engine::DecodePaddingMode::Indifferent),
        );
        let reference_optional = reference::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            reference::GeneralPurposeConfig::new()
                .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
        );
        let pairs = [
            (ours::STANDARD, reference::STANDARD),
            (ours::STANDARD_NO_PAD, reference::STANDARD_NO_PAD),
            (ours::URL_SAFE_NO_PAD, reference::URL_SAFE_NO_PAD),
            (ours_optional, reference_optional),
        ];
        for (our, reference) in pairs {
            for len in 0..256 {
                let bytes: Vec<_> = (0..len)
                    .map(|i| ((i * 197 + len * 73) % 256) as u8)
                    .collect();
                let encoded = our.encode(&bytes);
                assert_eq!(encoded, reference.encode(&bytes));
                assert_eq!(our.decode(&encoded).ok(), reference.decode(&encoded).ok());
            }
            let alphabet = b"ABZgz09+/-_= \n";
            for mut n in 0usize..alphabet.len().pow(4) {
                let mut candidate = [0; 4];
                for b in &mut candidate {
                    *b = alphabet[n % alphabet.len()];
                    n /= alphabet.len();
                }
                for len in 0..=4 {
                    let input = &candidate[..len];
                    assert_eq!(
                        our.decode(input).ok(),
                        reference.decode(input).ok(),
                        "input {input:?}"
                    );
                }
            }
        }
    }
}
