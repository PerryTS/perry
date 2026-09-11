pub(crate) fn compute_utf16_len_wtf8(bytes: &[u8]) -> u32 {
    let mut count = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            count += 1;
            i += 1;
        } else if b < 0xC0 {
            // continuation byte in lead position — skip
            i += 1;
        } else if b < 0xE0 {
            count += 1;
            i += 2;
        } else if b < 0xF0 {
            // 3-byte sequence: BMP codepoint or WTF-8 lone surrogate → 1 unit
            count += 1;
            i += 3;
        } else {
            // 4-byte sequence: astral codepoint → 2 UTF-16 units
            count += 2;
            i += 4;
        }
    }
    count
}

// Proposal only: the production caller would first prove that input is the
// exact live source-string payload and that its header count is authoritative.
fn hint(input: &[u8], start: usize, end: usize, total: u32) -> Option<u32> {
    if start > end || end > input.len() { return None; }
    let outside = start + input.len() - end;
    if outside > 256 || !input[..start].is_ascii() || !input[end..].is_ascii() { return None; }
    let token = &input[start..end];
    // Conservatively forbid a scalar WTF-8 lead claiming a byte beyond the
    // token boundary. A skipped lead may reject a safe case; it cannot admit
    // an unsafe one. No Unicode validity assumption is made.
    for distance in 1..=3.min(token.len()) {
        let byte = token[token.len() - distance];
        let width = if byte >= 0xf0 { 4 } else if byte >= 0xe0 { 3 } else if byte >= 0xc0 { 2 } else { 0 };
        if width > distance { return None; }
    }
    total.checked_sub(outside as u32)
}
fn check(token: &[u8], trials: &mut u64, accepted: &mut u64) {
    let mut input = br#"{"id":1,"text":""#.to_vec();
    let start=input.len();input.extend_from_slice(token);let end=input.len();input.extend_from_slice(br#"","next":7}"#);
    if let Some(value)=hint(&input,start,end,compute_utf16_len_wtf8(&input)) {
        assert_eq!(value,compute_utf16_len_wtf8(token),"{:x?}",token);*accepted+=1;
    }
    *trials+=1;
}
fn main() {
    let(mut trials,mut accepted)=(0,0);
    check(&[],&mut trials,&mut accepted);
    for a in 0..=255u8 { check(&[a],&mut trials,&mut accepted);for b in 0..=255u8 {check(&[a,b],&mut trials,&mut accepted);} }
    let alphabet=[0x00,b'a',0x80,0xbf,0xc0,0xdf,0xe0,0xef,0xf0,0xff];
    for mut n in 0..1_000_000usize {
        let mut token=[0u8;6];for b in &mut token {*b=alphabet[n%10];n/=10;}check(&token,&mut trials,&mut accepted);
    }
    let mut state=0x983482119823fab1u64;
    let mut token=[0u8;128];
    for round in 0..2_000_000usize {
        for b in &mut token[..round%129] {state^=state<<13;state^=state>>7;state^=state<<17;*b=state as u8;}
        check(&token[..round%129],&mut trials,&mut accepted);
    }
    for token in ["Grüße東京🙂","\u{d7ff}\u{e000}\u{10ffff}","ASCII text"] {check(token.as_bytes(),&mut trials,&mut accepted);}
    assert!(hint(&[0xc3,0xbc,b'"',b'a',b'"'],3,4,4).is_none());
    assert!(hint(&[b'a';300],270,280,300).is_none());
    assert!(hint(b"abc",2,1,3).is_none());
    assert!(hint(b"abc",1,4,3).is_none());
    assert!(hint(b"abc",1,2,0).is_none());
    println!("{trials} byte cases checked; {accepted} safely admitted, {} conservatively rejected",trials-accepted);
}
