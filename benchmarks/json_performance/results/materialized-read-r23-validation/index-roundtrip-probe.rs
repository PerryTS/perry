const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const INT32_MASK: u64 = 0x0000_0000_FFFF_FFFF;
const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
struct JSValue {bits:u64}
impl JSValue {fn from_bits(bits:u64)->Self {Self {bits}}
pub fn is_number(&self) -> bool {
        // Perry-owned tags occupy the positive qNaN band 0x7FF9..=0x7FFF.
        // Keep IEEE f64 values, including canonical qNaN 0x7FF8 and negative
        // NaN payloads, classified as numbers.
        let tag = self.bits & TAG_MASK;
        !(SHORT_STRING_TAG..=STRING_TAG).contains(&tag)
    }
pub fn is_int32(&self) -> bool {
        (self.bits & !INT32_MASK) == INT32_TAG
    }
pub fn as_int32(&self) -> i32 {
        debug_assert!(self.is_int32(), "Value is not an int32");
        (self.bits & INT32_MASK) as i32
    }
}
#[no_mangle]
#[inline(never)]
pub fn original_index(value: f64) -> Option<u32> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        return (js.as_int32() >= 0).then_some(js.as_int32() as u32);
    }
    (js.is_number()
        && value.is_finite()
        && value >= 0.0
        && value.fract() == 0.0
        && value <= (u32::MAX - 1) as f64)
        .then_some(value as u32)
}
#[no_mangle]
#[inline(never)]
pub fn proposed_index(value:f64)->Option<u32> {
 let js=JSValue::from_bits(value.to_bits());
 if js.is_int32() { return (js.as_int32()>=0).then_some(js.as_int32() as u32); }
 let index=value as u32;
 (index!=u32::MAX && f64::from(index)==value).then_some(index)
}
fn main() {
 let mut total=0u64;let mut accepted=0u64;
 let mut check=|bits:u64| {
  let value=std::hint::black_box(f64::from_bits(bits));
  let old=original_index(value);let new=proposed_index(value);
  assert_eq!(new,old,"bits={bits:016x}, value={value:?}");
  total+=1;accepted+=u64::from(old.is_some());
 };
 for top in 0..=u16::MAX {
  for low in [0,1,0xff,0xffff,0x7fff_ffff,0xffff_ffff,0x1234_5678_9abc,0xffff_ffff_ffff] {check((u64::from(top)<<48)|low);}
 }
 for n in 0..1_000_000u64 {
  let bits=(n as f64).to_bits();
  check(bits);check(bits.wrapping_add(1));check(bits.wrapping_sub(1));
  check(INT32_TAG|n);check(INT32_TAG|((-(n as i32)) as u32 as u64));
 }
 for value in [0.0,-0.0,1.0,-1.0,0.5,-0.5,f64::MIN_POSITIVE,f64::INFINITY,f64::NEG_INFINITY,f64::NAN,2147483647.0,2147483648.0,4294967294.0,4294967295.0,4294967296.0] {
  let bits=value.to_bits();for delta in -32i64..=32 {check(bits.wrapping_add_signed(delta));}
 }
 let mut state=0xd842_ca73_5190_3f21u64;
 for _ in 0..10_000_000 {state^=state<<13;state^=state>>7;state^=state<<17;check(state);}
 assert!(accepted>2_000_000);assert!(total-accepted>10_000_000);
 for (value,expected) in [(0.0,Some(0)),(-0.0,Some(0)),(0.5,None),(-1.0,None),(4294967294.0,Some(4294967294)),(4294967295.0,None),(f64::NAN,None),(f64::INFINITY,None)] {assert_eq!(proposed_index(value),expected);}
 println!("verified {total} bit patterns: {accepted} accepted, {} rejected",total-accepted);
}
