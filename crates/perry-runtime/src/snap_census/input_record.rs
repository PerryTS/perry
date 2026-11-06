//! Ordered evidence for execution-prefix eligibility. This module is only
//! compiled with the Linux instruments runner; it never decides production facts.
use super::{read_ir, top_init, State};
use std::fmt::Write as _;

#[derive(Default)]
pub(super) struct Record {
    pub seq: u64,
    pub events: Vec<Event>,
    pub writes: Vec<Option<Write>>,
    pub watching: bool,
    pub call_depth: u32,
    pub allocated_bytes: u64,
}
pub(super) struct Write {
    pub seq: u64,
    pub ir: u64,
    pub word: usize,
    pub pc: usize,
}
pub(super) struct Event {
    seq: u64,
    ir: u64,
    allocated_bytes: u64,
    init: u32,
    stack: Vec<(u32, u32, u8)>,
    pcs: [usize; 6],
    op: String,
    value: String,
}
pub(super) fn event(st: &mut State, op: &str, value: String) -> u64 {
    let ir = read_ir(st.perf_fd);
    let init = top_init(st);
    st.inputs.seq += 1;
    let seq = st.inputs.seq;
    st.inputs.writes.resize_with(st.inits.len(), || None);
    let mut pcs = [0usize; crate::error::MAX_CAPTURED_FRAMES];
    let n = crate::error::capture_ips(&mut pcs);
    let mut six = [0; 6];
    for (to, from) in six.iter_mut().zip(pcs[..n].iter().skip(1)) {
        *to = *from;
    }
    let stack = st
        .frames
        .iter()
        .map(|f| {
            (
                f.init,
                st.inits[f.init as usize].parts as u32,
                u8::from(f.kind == super::FrameKind::Job),
            )
        })
        .collect();
    st.inputs.events.push(Event {
        seq,
        ir,
        allocated_bytes: st.inputs.allocated_bytes,
        init,
        stack,
        pcs: six,
        op: op.to_owned(),
        value,
    });
    let saved_errno = unsafe { *libc::__errno_location() };
    unsafe {
        libc::syscall(
            libc::SYS_lseek,
            -1 as libc::c_int,
            seq as libc::off_t,
            0x5321 as libc::c_int,
        );
    }
    unsafe {
        *libc::__errno_location() = saved_errno;
    }
    seq
}
pub(super) fn watch(st: &State) -> bool {
    let id = top_init(st) as usize;
    st.inputs.call_depth == 0
        && id != 0
        && st.inits[id].parts >= 2
        && st.inputs.writes.get(id).is_some_and(Option::is_none)
}
/// Called after the census has excluded bookkeeping and untracked binary
/// words. A signal handler must not allocate or format strings here.
pub(super) fn changed_word(st: &mut State, word: usize, pc: usize) {
    let id = top_init(st) as usize;
    if !watch(st) {
        return;
    }
    let external = st.globals.contains(word as u64)
        || (st.realm.0 <= word && word < st.realm.1)
        || super::object_start(st, word)
            .and_then(|h| st.born_by.get(h as u64))
            .is_some_and(|owner| owner as usize != id);
    if !external {
        return;
    }
    st.inputs.seq += 1;
    st.inputs.writes[id] = Some(Write {
        seq: st.inputs.seq,
        ir: read_ir(st.perf_fd),
        word,
        pc,
    });
}
pub(super) fn write(st: &State) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut rows: Vec<(u64, String)> = Vec::new();
    let mut out = std::io::BufWriter::new(std::fs::File::create(format!("{}.inputs.tsv", st.out))?);
    writeln!(
        out,
        "seq\tir\tinit\tstack_parts\top\tvalue_hex\tpcs\tallocated_bytes"
    )?;
    for e in &st.inputs.events {
        let stack = e
            .stack
            .iter()
            .map(|(i, p, k)| format!("{i}:{p}:{k}"))
            .collect::<Vec<_>>()
            .join(",");
        let pcs = e
            .pcs
            .iter()
            .map(|p| format!("{p:x}"))
            .collect::<Vec<_>>()
            .join(",");
        rows.push((
            e.seq,
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                e.seq,
                e.ir,
                e.init,
                stack,
                e.op,
                hex(e.value.as_bytes()),
                pcs,
                e.allocated_bytes
            ),
        ));
    }
    for (id, hit) in st.inputs.writes.iter().enumerate() {
        if let Some(w) = hit {
            rows.push((
                w.seq,
                format!(
                    "{}\t{}\t{}\t\tfirst_changed_tracked_word\t{}\t{:x}\t",
                    w.seq,
                    w.ir,
                    id,
                    hex(format!("{:x}", w.word).as_bytes()),
                    w.pc
                ),
            ));
        }
    }
    rows.sort_unstable_by_key(|r| r.0);
    for (_, line) in rows {
        writeln!(out, "{line}")?;
    }
    out.flush()
}
pub(super) fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}
pub trait Value {
    fn input_value(&self) -> String;
}
macro_rules! debug_value {
    ($($t:ty),*) => { $(impl Value for $t {
        fn input_value(&self) -> String { format!("{self:?}") }
    })* }
}
debug_value!(
    (),
    bool,
    u8,
    u16,
    u32,
    u64,
    usize,
    i8,
    i16,
    i32,
    i64,
    isize,
    u128,
    String
);
impl Value for &str {
    fn input_value(&self) -> String {
        format!("string:{}", hex(self.as_bytes()))
    }
}
impl Value for f64 {
    fn input_value(&self) -> String {
        format!("f64:{:016x}", self.to_bits())
    }
}
impl<T: Value> Value for Option<T> {
    fn input_value(&self) -> String {
        match self {
            None => "None".into(),
            Some(v) => format!("Some({})", v.input_value()),
        }
    }
}
impl<T: Value> Value for Vec<T> {
    fn input_value(&self) -> String {
        format!(
            "[{}]",
            self.iter()
                .map(Value::input_value)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}
impl<T: Value, const N: usize> Value for [T; N] {
    fn input_value(&self) -> String {
        format!(
            "[{}]",
            self.iter()
                .map(Value::input_value)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}
impl<A: Value, B: Value> Value for (A, B) {
    fn input_value(&self) -> String {
        format!("({},{})", self.0.input_value(), self.1.input_value())
    }
}
impl Value for *mut crate::StringHeader {
    fn input_value(&self) -> String {
        if self.is_null() {
            return "null".into();
        }
        unsafe {
            let bytes =
                std::slice::from_raw_parts(self.add(1).cast::<u8>(), (**self).byte_len as usize);
            format!("string:{}", hex(bytes))
        }
    }
}
impl Value for *mut crate::ArrayHeader {
    fn input_value(&self) -> String {
        if self.is_null() {
            return "null".into();
        }
        let len = unsafe { (**self).length };
        let mut out = String::from("[");
        for i in 0..len {
            let value = crate::array::js_array_get_f64_unchecked(*self, i).to_bits();
            if i != 0 {
                out.push(',');
            }
            if value >> 48 == 0x7fff {
                let ptr = (value & 0x0000_ffff_ffff_ffff) as *mut crate::StringHeader;
                out.push_str(&ptr.input_value());
            } else {
                let _ = write!(out, "bits:{value:016x}");
            }
        }
        out.push(']');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_scalar_and_absence_values() {
        assert_ne!(0.0f64.input_value(), (-0.0f64).input_value());
        assert_ne!(
            Option::<String>::None.input_value(),
            Some(String::new()).input_value()
        );
        assert_eq!(hex(&[0, 0xff, 10]), "00ff0a");
    }
}

impl Value for *mut crate::buffer::BufferHeader {
    fn input_value(&self) -> String {
        if self.is_null() {
            return "null".into();
        }
        let value = f64::from_bits(crate::JSValue::pointer(*self as *const u8).bits());
        crate::buffer::bytes::no_gc(|scope| match crate::buffer::bytes::bytes(value, scope) {
            Ok(bytes) => format!("bytes:{}", hex(bytes)),
            Err(_) => "unsupported_buffer_payload".into(),
        })
    }
}
