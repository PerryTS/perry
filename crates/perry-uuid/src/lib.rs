//! The two UUID versions Perry generates. Features default to empty: mail
//! selects v4; node:crypto selects v4+v7. Entropy comes from the OS.
#[cfg(any(feature = "v4", feature = "v7"))]
fn random_bytes() -> [u8; 16] {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).expect("OS entropy unavailable for UUID generation");
    bytes
}
#[cfg(any(feature = "v4", feature = "v7"))]
fn format(bytes: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(36);
    for (i, b) in bytes.into_iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(char::from(HEX[(b >> 4) as usize]));
        out.push(char::from(HEX[(b & 15) as usize]));
    }
    out
}
#[cfg(feature = "v4")]
pub fn v4() -> String {
    let mut bytes = random_bytes();
    bytes[6] = (bytes[6] & 15) | 0x40;
    bytes[8] = (bytes[8] & 63) | 0x80;
    format(bytes)
}
#[cfg(feature = "v7")]
#[derive(Default)]
struct Sequence {
    millis: u64,
    counter: u64,
}
#[cfg(feature = "v7")]
impl Sequence {
    // A 42-bit randomly seeded counter occupies rand_a and the high 30 bits
    // of rand_b. Keep the low 32 bits random. Roll the logical timestamp
    // forward on overflow; clock rollback never reverses generation order.
    fn next(&mut self, millis: u64, random: [u8; 16]) -> [u8; 16] {
        if millis > self.millis || self.counter == 0 {
            self.millis = millis;
            self.counter = u64::from_be_bytes(random[..8].try_into().unwrap()) & ((1 << 41) - 1);
        } else if self.counter == (1 << 42) - 1 {
            self.millis += 1;
            self.counter = 0;
        }
        self.counter += 1;
        let mut bytes = random;
        bytes[..6].copy_from_slice(&self.millis.to_be_bytes()[2..]);
        bytes[6] = 0x70 | ((self.counter >> 38) as u8 & 15);
        bytes[7] = (self.counter >> 30) as u8;
        bytes[8] = 0x80 | ((self.counter >> 24) as u8 & 63);
        bytes[9] = (self.counter >> 16) as u8;
        bytes[10] = (self.counter >> 8) as u8;
        bytes[11] = self.counter as u8;
        bytes
    }
}
#[cfg(feature = "v7")]
pub fn v7() -> String {
    use std::{
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };
    static SEQUENCE: Mutex<Sequence> = Mutex::new(Sequence {
        millis: 0,
        counter: 0,
    });
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("UUID clock precedes Unix epoch")
        .as_millis() as u64;
    let random = random_bytes();
    let bytes = SEQUENCE.lock().unwrap().next(millis, random);
    format(bytes)
}
#[cfg(test)]
mod tests {
    #[cfg(feature = "v4")]
    #[test]
    fn v4_layout_and_fresh_entropy() {
        let a = super::v4();
        let b = super::v4();
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert!(matches!(a.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        assert_ne!(a, b);
    }
    #[cfg(feature = "v7")]
    #[test]
    fn v7_clock_rollback_and_counter_overflow() {
        let mut seq = super::Sequence::default();
        let a = seq.next(1700000000123, [3; 16]);
        let b = seq.next(1700000000123, [0; 16]);
        let c = seq.next(1, [0; 16]);
        assert!(a < b && b < c);
        assert_eq!(a[6] >> 4, 7);
        assert_eq!(a[8] >> 6, 2);
        assert_eq!(&a[..6], &1700000000123u64.to_be_bytes()[2..]);
        seq.counter = (1 << 42) - 1;
        let d = seq.next(1, [0; 16]);
        assert!(c < d);
        assert_eq!(seq.millis, 1700000000124);
    }
}
