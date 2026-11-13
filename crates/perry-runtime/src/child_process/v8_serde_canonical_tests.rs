//! P3 B1 audit (perry-wpfacts/NUM_SEEDS.md, "int32-box producers"): a V8
//! wire int32 (`'I'` + zigzag varint), as `v8.serialize` writes for small
//! integers, deserializes to the raw double, never an INT32-tagged word.

use super::{v8_deserialize, v8_serialize};

/// `0xFF 0x0F` (version 15), then `'I'` and the zigzag varint of `n`.
fn wire_int32(n: i32) -> Vec<u8> {
    let mut z = ((n << 1) ^ (n >> 31)) as u32;
    let mut out = vec![0xFF, 0x0F, b'I'];
    loop {
        let byte = (z & 0x7F) as u8;
        z >>= 7;
        if z == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out
}

#[test]
fn a_wire_int32_deserializes_to_the_raw_double() {
    for n in [0, 5, -7, 1, -1, i32::MAX, i32::MIN] {
        let v = v8_deserialize(&wire_int32(n));
        assert_eq!(v.to_bits(), (n as f64).to_bits(), "wire int32 {n}");
    }
}

#[test]
fn serialize_then_deserialize_keeps_small_integers_raw() {
    for n in [0.0, 5.0, -7.0, 2147483647.0, -2147483648.0] {
        let bytes = v8_serialize(n);
        // Skip the version tag and its varint.
        let mut at = 1;
        while bytes[at] & 0x80 != 0 {
            at += 1;
        }
        assert_eq!(
            bytes.get(at + 1),
            Some(&b'I'),
            "{n} is written as a wire int32"
        );
        assert_eq!(v8_deserialize(&bytes).to_bits(), f64::to_bits(n), "{n}");
    }
}
