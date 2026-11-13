//! Ordered execution-prefix evidence, streamed through native storage.
use super::{read_ir, top_init, State};
use std::fmt::Write as _;

pub(super) struct Record {
    pub seq: u64,
    pub writes: Vec<Option<Write>>,
    pub watching: bool,
    pub watch_enabled: bool,
    pub call_depth: u32,
    pub allocated_bytes: u64,
    fd: i32,
}
impl Default for Record {
    fn default() -> Self {
        Self {
            seq: 0,
            writes: Vec::new(),
            watching: false,
            watch_enabled: true,
            call_depth: 0,
            allocated_bytes: 0,
            fd: -1,
        }
    }
}
pub(super) struct Write;

pub(super) fn open(st: &mut State) -> std::io::Result<()> {
    use std::os::fd::IntoRawFd;
    let saved = unsafe { *libc::__errno_location() };
    let _scope = Scope::enter();
    st.inputs.fd = std::fs::File::create(format!("{}.inputs.tsv", st.out))?.into_raw_fd();
    emit(
        st.inputs.fd,
        b"seq\tir\tinit\tstack_parts\top\tvalue_hex\tpcs\tallocated_bytes\n",
    );
    unsafe {
        *libc::__errno_location() = saved;
    }
    Ok(())
}

/// The companion records these intervals as diagnostic execution. The marker
/// and file writes preserve errno and never enter a JS allocator.
pub(crate) fn logger_scope(enter: bool) {
    if enter {
        super::logging_scope(true);
    }
    unsafe {
        let saved = *libc::__errno_location();
        libc::syscall(
            libc::SYS_lseek,
            -1 as libc::c_int,
            u64::from(enter),
            0x5322 as libc::c_int,
        );
        *libc::__errno_location() = saved;
    }
    if !enter {
        super::logging_scope(false);
    }
}
pub(crate) struct Scope;
impl Scope {
    pub(crate) fn enter() -> Self {
        logger_scope(true);
        Self
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        logger_scope(false);
    }
}
fn emit(fd: i32, bytes: &[u8]) {
    let saved = unsafe { *libc::__errno_location() };
    let mut at = 0;
    while at < bytes.len() {
        let n = unsafe { libc::write(fd, bytes.as_ptr().add(at).cast(), bytes.len() - at) };
        if n > 0 {
            at += n as usize;
        } else if n < 0 && unsafe { *libc::__errno_location() } == libc::EINTR {
            continue;
        } else {
            let message = b"SNAP22 input record write failed\n";
            unsafe {
                libc::write(2, message.as_ptr().cast(), message.len());
                libc::_exit(92);
            }
        }
    }
    unsafe {
        *libc::__errno_location() = saved;
    }
}
pub(super) fn event(st: &mut State, op: &str, value: String) -> u64 {
    let saved = unsafe { *libc::__errno_location() };
    logger_scope(true);
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
    if st.p_reach_rule
        && super::is_entry(st, init)
        && !matches!(
            op,
            "init_enter"
                | "init_exit"
                | "entry_body"
                | "result"
                | "boundary"
                | "job_enter"
                | "job_exit"
                | "census_start"
                | "p_reach"
                | "p_entry"
                | "k2a"
                | "host_value.held"
        )
    {
        // SNAPK2b: the argv grain P-reach narrows (`init_enter`).
        let label = match op {
            "argv.read" if value.starts_with("escape") => "argv.escape",
            "host_value" if value.starts_with("what=argv;") => "host_value.argv",
            "compare_literal" if value.contains(";src=argv") => "compare_literal.argv",
            _ => op,
        };
        st.entry_rows.push((seq, label.to_owned()));
    }
    let mut row = format!("{seq}\t{ir}\t{init}\t");
    for (i, f) in st.frames.iter().enumerate() {
        if i != 0 {
            row.push(',');
        }
        let _ = write!(
            row,
            "{}:{}:{}",
            f.init,
            st.inits[f.init as usize].parts,
            u8::from(f.kind == super::FrameKind::Job)
        );
    }
    let _ = write!(row, "\t{op}\t{}\t", hex(value.as_bytes()));
    for (i, pc) in six.iter().enumerate() {
        if i != 0 {
            row.push(',');
        }
        let _ = write!(row, "{pc:x}");
    }
    let _ = writeln!(row, "\t{}", st.inputs.allocated_bytes);
    emit(st.inputs.fd, row.as_bytes());
    logger_scope(false);
    unsafe {
        libc::syscall(
            libc::SYS_lseek,
            -1 as libc::c_int,
            seq as libc::off_t,
            0x5321 as libc::c_int,
        );
        *libc::__errno_location() = saved;
    }
    seq
}
pub(super) fn watch(st: &State) -> bool {
    let id = top_init(st) as usize;
    st.inputs.watch_enabled
        && st.inputs.call_depth == 0
        && id != 0
        && st.inits[id].parts >= 2
        && st.inputs.writes.get(id).is_some_and(Option::is_none)
}

struct SignalRow<const N: usize> {
    bytes: [u8; N],
    len: usize,
}
impl<const N: usize> std::fmt::Write for SignalRow<N> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        if self.len + value.len() > self.bytes.len() {
            return Err(std::fmt::Error);
        }
        self.bytes[self.len..self.len + value.len()].copy_from_slice(value.as_bytes());
        self.len += value.len();
        Ok(())
    }
}
/// No allocation, formatting heap, JS API or deferred payload is permitted in
/// the fault handler. An oversized frame record fails explicitly, never truncates.
pub(super) fn remember_write(st: &mut State, word: usize, pc: usize) {
    let saved = unsafe { *libc::__errno_location() };
    let id = top_init(st) as usize;
    st.inputs.seq += 1;
    st.inputs.writes[id] = Some(Write);
    let ir = read_ir(st.perf_fd);
    logger_scope(true);
    let mut row = SignalRow {
        bytes: [0; 65536],
        len: 0,
    };
    let result = (|| -> std::fmt::Result {
        write!(row, "{}\t{ir}\t{id}\t", st.inputs.seq)?;
        for (i, f) in st.frames.iter().enumerate() {
            if i != 0 {
                row.write_char(',')?;
            }
            write!(
                row,
                "{}:{}:{}",
                f.init,
                st.inits[f.init as usize].parts,
                u8::from(f.kind == super::FrameKind::Job)
            )?;
        }
        row.write_str("\tfirst_changed_tracked_word\t")?;
        let mut text = SignalRow {
            bytes: [0; 16],
            len: 0,
        };
        write!(text, "{word:x}")?;
        for byte in &text.bytes[..text.len] {
            write!(row, "{byte:02x}")?;
        }
        writeln!(row, "\t{pc:x}\t{}", st.inputs.allocated_bytes)
    })();
    if result.is_err() {
        unsafe {
            libc::_exit(93);
        }
    }
    emit(st.inputs.fd, &row.bytes[..row.len]);
    logger_scope(false);
    unsafe {
        *libc::__errno_location() = saved;
    }
}
pub(super) fn changed_word(st: &mut State, word: usize, pc: usize) {
    let id = top_init(st) as usize;
    if !watch(st) {
        return;
    }
    let external = st.globals.contains(word as u64)
        || (st.realm.0 <= word && word < st.realm.1)
        || st.cells.object_start(word)
            .and_then(|h| st.cells.born_by.get(h as u64))
            .is_some_and(|(owner, _)| owner as usize != id);
    if external {
        remember_write(st, word, pc);
    }
}
pub(super) fn write(_st: &State) -> std::io::Result<()> {
    // Rows are unbuffered write-all calls, including the fault-handler rows.
    Ok(())
}

/// Used by libc callbacks, including callbacks from inside mimalloc. No
/// heap allocation, JS API, backtrace cache, or native allocator is entered.
pub(super) fn native_event(st: &mut State, op: &str, value: std::fmt::Arguments<'_>) {
    let saved = unsafe { *libc::__errno_location() };
    let _scope = Scope::enter();
    let ir = read_ir(st.perf_fd);
    st.inputs.seq += 1;
    let seq = st.inputs.seq;
    let mut text = SignalRow {
        bytes: [0; 65536],
        len: 0,
    };
    if text.write_fmt(value).is_err() {
        unsafe {
            libc::_exit(93);
        }
    }
    let mut six = [0usize; 6];
    if super::c25() {
        if super::funnel() != 0 && !op.starts_with("getenv") {
            let _ = write!(text, ";funnel={}", super::funnel_name(super::funnel()));
        }
        row_pcs(&mut six);
    }
    let mut row = SignalRow {
        bytes: [0; 131072],
        len: 0,
    };
    let result = (|| -> std::fmt::Result {
        write!(row, "{seq}\t{ir}\t{}\t", top_init(st))?;
        for (i, f) in st.frames.iter().enumerate() {
            if i != 0 {
                row.write_char(',')?;
            }
            write!(
                row,
                "{}:{}:{}",
                f.init,
                st.inits[f.init as usize].parts,
                u8::from(f.kind == super::FrameKind::Job)
            )?;
        }
        write!(row, "\t{op}\t")?;
        for b in &text.bytes[..text.len] {
            write!(row, "{b:02x}")?;
        }
        row.write_char('\t')?;
        for (i, pc) in six.iter().enumerate() {
            if i != 0 {
                row.write_char(',')?;
            }
            write!(row, "{pc:x}")?;
        }
        writeln!(row, "\t{}", st.inputs.allocated_bytes)
    })();
    if result.is_err() {
        unsafe {
            libc::_exit(93);
        }
    }
    emit(st.inputs.fd, &row.bytes[..row.len]);
    unsafe {
        libc::syscall(
            libc::SYS_lseek,
            -1 as libc::c_int,
            seq as libc::off_t,
            0x5321 as libc::c_int,
        );
        *libc::__errno_location() = saved;
    }
}
pub(super) struct Hex<'a>(pub &'a [u8]);
impl std::fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}
pub(super) fn env_rows(st: &State) -> std::io::Result<Vec<(u32, String, Option<String>)>> {
    use std::io::{BufRead, Error, ErrorKind};
    fn unhex(s: &str) -> std::io::Result<Vec<u8>> {
        if s.len() % 2 != 0 {
            return Err(Error::new(ErrorKind::InvalidData, "odd hex"));
        }
        (0..s.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&s[i..i + 2], 16)
                    .map_err(|_| Error::new(ErrorKind::InvalidData, "invalid hex"))
            })
            .collect()
    }
    let mut rows = Vec::new();
    for line in
        std::io::BufReader::new(std::fs::File::open(format!("{}.inputs.tsv", st.out))?).lines()
    {
        let line = line?;
        let fields: Vec<_> = line.split('\t').collect();
        if fields.get(4) != Some(&"getenv") && fields.get(4) != Some(&"getenv.program") {
            continue;
        }
        let value = String::from_utf8(unhex(fields[5])?)
            .map_err(|_| Error::new(ErrorKind::InvalidData, "bad env row"))?;
        let (name, value) = value
            .split_once('=')
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing env separator"))?;
        let value = if value == "unset" {
            None
        } else {
            Some(String::from_utf8_lossy(&unhex(value)?).into_owned())
        };
        rows.push((
            fields[2]
                .parse()
                .map_err(|_| Error::new(ErrorKind::InvalidData, "bad init"))?,
            String::from_utf8_lossy(&unhex(name)?).into_owned(),
            value,
        ));
    }
    Ok(rows)
}
/// SNAP25: env names read inside a program-input funnel (`getenv.program`):
/// guard members whatever their prefix.
pub(super) fn program_env_names(st: &State) -> Vec<String> {
    use std::io::BufRead;
    let mut out = Vec::new();
    let Ok(f) = std::fs::File::open(format!("{}.inputs.tsv", st.out)) else {
        return out;
    };
    for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.get(4) == Some(&"getenv.program") {
            let raw = fields[5];
            let bytes: Vec<u8> = (0..raw.len() / 2)
                .filter_map(|i| u8::from_str_radix(&raw[2 * i..2 * i + 2], 16).ok())
                .collect();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let name = text.split_once('=').map_or(text.as_str(), |(n, _)| n);
            let name: Vec<u8> = (0..name.len() / 2)
                .filter_map(|i| u8::from_str_radix(&name[2 * i..2 * i + 2], 16).ok())
                .collect();
            out.push(String::from_utf8_lossy(&name).into_owned());
        }
    }
    out
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
        if unsafe {
            crate::array::array_has_element_descriptors_resolved(*self) || len > (**self).capacity
        } {
            return "unsupported_array_payload".into();
        }
        let mut out = String::from("[");
        for i in 0..len {
            let mut value = unsafe {
                (crate::array::array_elements_ptr(*self) as *const f64)
                    .add(i as usize)
                    .read()
                    .to_bits()
            };
            if value == crate::value::TAG_HOLE {
                value = 0x7ffc_0000_0000_0001;
            }
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

/// The six return addresses above the native read, or none inside the
/// walker funnel: this thread holds the walker's lock there, and a capture
/// (which walks through the same walker) would wait on it forever (SNAPK3:
/// js_throw -> find_handler -> decode_fde_row -> mimalloc arena free ->
/// clock_gettime -> here). The row keeps `funnel=unwind`, its class.
fn row_pcs(six: &mut [usize; 6]) {
    if super::funnel() == crate::snap_census::Funnel::Unwind as u8 {
        return;
    }
    let mut pcs = [0usize; crate::error::MAX_CAPTURED_FRAMES];
    let n = crate::error::capture_ips(&mut pcs);
    for (to, from) in six.iter_mut().zip(pcs[..n].iter().skip(1)) {
        *to = *from;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A native read inside the walker funnel returns without walking.
    /// Red (capture regardless of the funnel): the capture waits on the
    /// walker lock this thread holds, and the receiver times out.
    #[test]
    fn row_pcs_inside_walker_lock_returns() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            crate::eh_walker::with_walker_locked(|| {
                let unwind = crate::snap_census::Funnel::Unwind as u8;
                let prev = super::super::funnel_set_for_test(unwind);
                let mut six = [0usize; 6];
                row_pcs(&mut six);
                super::super::funnel_set_for_test(prev);
                let _ = tx.send(six);
            });
        });
        let six = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("row_pcs re-entered the walker lock");
        assert_eq!(six, [0; 6]);
    }
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
