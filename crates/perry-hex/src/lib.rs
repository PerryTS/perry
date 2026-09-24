//! Strict byte codecs. These deliberately do not implement Node Buffer's
//! permissive truncation semantics. No runtime, global state or dependencies.
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    OddLength,
    InvalidDigit { byte: u8, index: usize },
}
impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OddLength => f.write_str("Odd number of digits"),
            Self::InvalidDigit { byte, index } => write!(
                f,
                "Invalid character {:?} at position {index}",
                char::from(*byte)
            ),
        }
    }
}
impl std::error::Error for DecodeError {}

pub fn encode(input: impl AsRef<[u8]>) -> String {
    encode_with(input.as_ref(), b"0123456789abcdef")
}
pub fn encode_upper(input: impl AsRef<[u8]>) -> String {
    encode_with(input.as_ref(), b"0123456789ABCDEF")
}
fn encode_with(input: &[u8], alphabet: &[u8; 16]) -> String {
    let mut out = String::with_capacity(input.len().saturating_mul(2));
    for &b in input {
        out.push(char::from(alphabet[(b >> 4) as usize]));
        out.push(char::from(alphabet[(b & 15) as usize]));
    }
    out
}
pub fn decode(input: impl AsRef<[u8]>) -> Result<Vec<u8>, DecodeError> {
    let input = input.as_ref();
    if input.len() % 2 != 0 {
        return Err(DecodeError::OddLength);
    }
    let digit = |i: usize| match input[i] {
        b'0'..=b'9' => Ok(input[i] - b'0'),
        b'a'..=b'f' => Ok(input[i] - b'a' + 10),
        b'A'..=b'F' => Ok(input[i] - b'A' + 10),
        b => Err(DecodeError::InvalidDigit { byte: b, index: i }),
    };
    (0..input.len())
        .step_by(2)
        .map(|i| Ok(digit(i)? * 16 + digit(i + 1)?))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vectors_and_invalid_inputs() {
        assert_eq!(encode([0, 15, 128, 255]), "000f80ff");
        assert_eq!(encode_upper([0, 15, 128, 255]), "000F80FF");
        assert_eq!(decode("aBcD"), Ok(vec![171, 205]));
        assert_eq!(decode("f"), Err(DecodeError::OddLength));
        assert!(decode("0 ").is_err());
        assert!(decode("é").is_err());
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(encode(&all)).unwrap(), all);
    }
}
