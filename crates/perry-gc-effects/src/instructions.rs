//! The Python analyzer's instruction and reaching-definition rules, in AT&T
//! x86 and LLVM AArch64 syntax. LLVM MC supplies the same printer as objdump.
use crate::graph::Node;
use std::collections::{BTreeSet, HashMap, HashSet};

macro_rules! rx {
    ($s:expr) => {{
        static RX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        RX.get_or_init(|| regex::Regex::new($s).expect("constant instruction regex"))
    }};
}
pub(crate) use rx;
pub(crate) fn starts(s: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| s.starts_with(p))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arch {
    X86,
    Arm64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Elf,
    MachO,
    Coff,
}
impl Format {
    pub fn norm_owned(self, mut name: String) -> String {
        if self == Self::MachO && name.starts_with('_') {
            name.remove(0);
        }
        if name.starts_with("__imp_") {
            name.drain(..6);
        }
        name
    }
    pub fn norm(self, name: &str) -> String {
        let name = if self == Self::MachO {
            name.strip_prefix('_').unwrap_or(name)
        } else {
            name
        };
        name.strip_prefix("__imp_").unwrap_or(name).to_owned()
    }
}
#[derive(Debug, Clone)]
pub(crate) struct Insn {
    pub addr: u64,
    pub mn: String,
    pub ops: String,
    pub relocs: Vec<(String, String)>,
}
impl Insn {
    pub fn new(addr: u64, text: &str) -> Self {
        let (mut mn, mut raw) = text
            .trim()
            .split_once(char::is_whitespace)
            .unwrap_or((text.trim(), ""));
        raw = raw.trim();
        while [
            "notrack", "lock", "rep", "repe", "repne", "repz", "repnz", "bnd", "data16", "cs", "ds",
        ]
        .contains(&mn)
            && !raw.is_empty()
        {
            (mn, raw) = raw.split_once(char::is_whitespace).unwrap_or((raw, ""));
            raw = raw.trim();
        }
        let ops = raw
            .split("//")
            .next()
            .unwrap()
            .split(" # ")
            .next()
            .unwrap()
            .split('<')
            .next()
            .unwrap()
            .trim()
            .to_owned();
        Self {
            addr,
            mn: mn.to_owned(),
            ops,
            relocs: Vec::new(),
        }
    }
}
pub(crate) fn x86_family(reg: &str) -> String {
    let reg = reg.trim_start_matches('%');
    if let Some(c) = rx!(r"^r(\d{1,2})[dwb]?$").captures(reg) {
        return format!("r{}", &c[1]);
    }
    let base = match reg {
        "al" | "ah" => "ax",
        "bl" | "bh" => "bx",
        "cl" | "ch" => "cx",
        "dl" | "dh" => "dx",
        "sil" => "si",
        "dil" => "di",
        "spl" => "sp",
        "bpl" => "bp",
        _ => {
            if starts(reg, &["r", "e"]) && reg.len() == 3 {
                &reg[1..]
            } else {
                reg
            }
        }
    };
    format!("r{base}")
}
fn a64_family(reg: &str) -> Option<String> {
    rx!(r"^[xw](\d{1,2})$")
        .captures(reg.trim())
        .map(|c| format!("x{}", &c[1]))
}
const X86_IMPLICIT: &[&str] = &["rax", "rcx", "rdx", "rsi", "rdi", "r11"];
const X86_CALLER_SAVED: &[&str] = &["rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11"];
const ARG_REGS: &[&str] = &[
    "rdi", "rsi", "rdx", "rcx", "r8", "r9", "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8",
];
const REGEX_REG: &str =
    r"%([re]?[a-z]{1,2}x?|r\d{1,2}[dwb]?|[re]?[sd]il?|[re]?[sb]pl?|[a-d][lh])\b";

fn writes(ins: &Insn, reg: &str, arch: Arch) -> bool {
    let mn = ins.mn.as_str();
    let ops = ins.ops.as_str();
    if arch == Arch::X86 {
        if mn.starts_with("call") {
            return X86_CALLER_SAVED.contains(&reg);
        }
        if starts(mn, &["j", "ret"]) {
            return false;
        }
        if starts(
            mn,
            &[
                "mul", "div", "imul", "idiv", "cqto", "cqo", "cltd", "cltq", "cwtl", "cpuid",
                "rdtsc", "rdtscp", "syscall", "rep", "movs", "stos", "lods", "scas", "cmps",
                "xgetbv", "rdrand", "rdseed", "xchg", "cmpxchg", "xadd",
            ],
        ) && X86_IMPLICIT.contains(&reg)
        {
            return true;
        }
        if starts(mn, &["xchg", "cmpxchg", "xadd"]) {
            let no_mem = rx!(r"\([^)]*\)").replace_all(ops, "");
            return rx!(REGEX_REG)
                .captures_iter(&no_mem)
                .any(|c| x86_family(&c[1]) == reg);
        }
        if mn.starts_with("pop") {
            return rx!(REGEX_REG)
                .captures(ops)
                .is_some_and(|c| x86_family(&c[1]) == reg);
        }
        if (mn.starts_with("cmp") && !starts(mn, &["cmps", "cmov"]))
            || starts(
                mn,
                &[
                    "test", "push", "ucomis", "comis", "vucomis", "vcomis", "nop",
                ],
            )
            || ["bt", "btl", "btq", "btw"].contains(&mn)
        {
            return false;
        }
        let mut depth = 0;
        let mut last = 0;
        for (i, c) in ops.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => last = i + 1,
                _ => {}
            }
        }
        let dest = ops[last..].trim();
        dest.starts_with('%')
            && rx!(REGEX_REG)
                .captures(dest)
                .is_some_and(|c| x86_family(&c[1]) == reg)
    } else {
        if ["bl", "blr"].contains(&mn) || mn.starts_with("blra") {
            return reg == "x30"
                || reg
                    .strip_prefix('x')
                    .and_then(|s| s.parse::<u32>().ok())
                    .is_some_and(|n| n < 19);
        }
        if (ops.contains("]!") || rx!(r"\],\s*#").is_match(ops))
            && rx!(r"\[(x\d+|sp)")
                .captures(ops)
                .is_some_and(|c| &c[1] != "sp" && &c[1] == reg)
        {
            return true;
        }
        if [
            "b", "br", "ret", "cbz", "cbnz", "tbz", "tbnz", "nop", "dmb", "dsb", "isb", "hint",
            "brk", "udf", "yield", "prfm", "prfum", "cmp", "cmn", "tst", "fcmp", "fcmpe", "ccmp",
            "ccmn", "fccmp", "fccmpe", "bti", "pacibsp", "autibsp", "paciasp", "autiasp", "retab",
            "retaa",
        ]
        .contains(&mn)
            || starts(
                mn,
                &[
                    "b.", "str", "stp", "stur", "stlr", "st1", "st2", "st3", "st4", "stnp", "sttr",
                    "stg", "stz",
                ],
            )
        {
            return false;
        }
        let nwrite = if starts(mn, &["ldp", "ldnp", "ldaxp", "ldxp", "casp"]) {
            2
        } else {
            1
        };
        ops.split('[')
            .next()
            .unwrap()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .take(nwrite)
            .any(|r| a64_family(r).as_deref() == Some(reg))
    }
}
fn switch(insns: &[Insn], i: usize, arch: Arch) -> bool {
    let window = &insns[i.saturating_sub(8)..i];
    let ops = &insns[i].ops;
    if arch == Arch::X86 {
        if rx!(r"\*-?(0x[0-9a-f]+)?\((%r\w+)?,%r\w+,8\)").is_match(ops) {
            return window.iter().any(|w| w.mn.starts_with("lea"));
        }
        let target = ops.trim_start_matches('*').trim();
        window
            .iter()
            .any(|w| w.mn.starts_with("add") && w.ops.ends_with(target))
            && window
                .iter()
                .any(|w| w.mn.starts_with("movslq") && w.ops.contains(",4)"))
    } else {
        let reg = ops.split(',').next().unwrap().trim();
        let prefix = format!("{reg},");
        window.iter().any(|w| {
            w.mn == "add"
                && w.ops.starts_with(&prefix)
                && (w.ops.contains("sxtw #2") || w.ops.contains("lsl #2"))
        }) && window
            .iter()
            .any(|w| w.mn == "adr" && w.ops.starts_with(&prefix))
    }
}
fn got_symbol(ins: &Insn, tlvp: bool) -> Option<String> {
    ins.relocs
        .iter()
        .find(|(r, t)| {
            if tlvp {
                r.contains("TLVP_LOAD_PAGEOFF")
            } else {
                ["GOTPCREL", "GOT_LOAD", "GOTPAGE", "LD64_GOT", "ADR_GOT"]
                    .iter()
                    .any(|x| r.contains(x))
                    || t.starts_with("__imp_")
            }
        })
        .map(|(_, t)| t.clone())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    None,
    Call,
    Cond,
    Jump,
    Exit,
}
fn flow(ins: &Insn, arch: Arch) -> (Flow, Option<u64>) {
    let mn = ins.mn.as_str();
    let tgt = if ins.relocs.is_empty() && !ins.ops.starts_with('*') {
        rx!(r"\b0x([0-9a-f]+)\b")
            .captures(&ins.ops)
            .and_then(|c| u64::from_str_radix(&c[1], 16).ok())
    } else {
        None
    };
    if arch == Arch::X86 {
        if mn.starts_with("call") {
            (Flow::Call, None)
        } else if starts(mn, &["ret", "ud2", "hlt"]) {
            (Flow::Exit, None)
        } else if mn.starts_with("jmp") {
            (
                if tgt.is_some() {
                    Flow::Jump
                } else {
                    Flow::Exit
                },
                tgt,
            )
        } else if mn.starts_with('j') {
            (Flow::Cond, tgt)
        } else {
            (Flow::None, None)
        }
    } else {
        if ["bl", "blr"].contains(&mn) || mn.starts_with("blra") {
            (Flow::Call, None)
        } else if ["ret", "retaa", "retab", "brk", "udf", "br"].contains(&mn)
            || mn.starts_with("bra")
        {
            (Flow::Exit, None)
        } else if mn == "b" {
            (
                if tgt.is_some() {
                    Flow::Jump
                } else {
                    Flow::Exit
                },
                tgt,
            )
        } else if starts(mn, &["b.", "cbz", "cbnz", "tbz", "tbnz"]) {
            (Flow::Cond, tgt)
        } else {
            (Flow::None, None)
        }
    }
}
struct Cfg<'a> {
    insns: &'a [Insn],
    starts: Vec<usize>,
    block_of: Vec<usize>,
    pred: Vec<BTreeSet<usize>>,
}
impl<'a> Cfg<'a> {
    fn new(insns: &'a [Insn], arch: Arch, switch_at: &HashSet<usize>) -> Self {
        let addr_ix: HashMap<_, _> = insns
            .iter()
            .enumerate()
            .map(|(i, ins)| (ins.addr, i))
            .collect();
        let mut leaders = BTreeSet::from([0]);
        let flows: Vec<_> = insns.iter().map(|ins| flow(ins, arch)).collect();
        for (k, (kind, tgt)) in flows.iter().enumerate() {
            if *kind == Flow::None {
                continue;
            }
            if k + 1 < insns.len() {
                leaders.insert(k + 1);
            }
            if let Some(t) = tgt.and_then(|t| addr_ix.get(&t)) {
                leaders.insert(*t);
            }
        }
        let starts: Vec<_> = leaders.into_iter().collect();
        let mut block_of = vec![0; insns.len()];
        for (b, st) in starts.iter().enumerate() {
            block_of[*st..starts.get(b + 1).copied().unwrap_or(insns.len())].fill(b);
        }
        let mut pred = vec![BTreeSet::new(); starts.len()];
        let mut sw = BTreeSet::new();
        for b in 0..starts.len() {
            let last = starts.get(b + 1).copied().unwrap_or(insns.len()) - 1;
            if switch_at.contains(&last) {
                sw.insert(b);
                continue;
            }
            let (kind, tgt) = flows[last];
            if [Flow::None, Flow::Call, Flow::Cond].contains(&kind) && b + 1 < starts.len() {
                pred[b + 1].insert(b);
            }
            if [Flow::Cond, Flow::Jump].contains(&kind) {
                if let Some(t) = tgt.and_then(|t| addr_ix.get(&t)) {
                    pred[block_of[*t]].insert(b);
                }
            }
        }
        for p in &mut pred {
            p.extend(&sw);
        }
        Self {
            insns,
            starts,
            block_of,
            pred,
        }
    }
    fn end(&self, b: usize) -> usize {
        self.starts.get(b + 1).copied().unwrap_or(self.insns.len())
    }
    fn padding(&self, b: usize) -> bool {
        self.insns[self.starts[b]..self.end(b)].iter().all(|w| {
            starts(&w.mn, &["nop", "data16", "xchg"])
                && (!w.mn.starts_with("xchg") || w.ops == "%ax, %ax")
        })
    }
    fn pinned(&self, i: usize, reg: &str, arch: Arch, depth: usize, tlvp: bool) -> Option<String> {
        let mut sym = None;
        let mut seen = HashSet::new();
        let mut work = vec![(self.block_of[i], i)];
        while let Some((b, upto)) = work.pop() {
            let mut found = false;
            for k in (self.starts[b]..upto).rev() {
                let ins = &self.insns[k];
                if !writes(ins, reg, arch) {
                    continue;
                }
                let mut g = if flow(ins, arch).0 == Flow::Call {
                    None
                } else {
                    got_symbol(ins, tlvp)
                };
                if g.is_none() && depth < 6 && !tlvp {
                    let src = if arch == Arch::X86 && ins.mn == "movq" {
                        rx!(r"^%(r\w+), %(r\w+)$")
                            .captures(&ins.ops)
                            .map(|c| x86_family(&c[1]))
                    } else if arch == Arch::Arm64 && ins.mn == "mov" {
                        rx!(r"^x(\d+), x(\d+)$")
                            .captures(&ins.ops)
                            .map(|c| format!("x{}", &c[2]))
                    } else {
                        None
                    };
                    if let Some(src) = src.filter(|s| s != reg) {
                        g = self.pinned(k, &src, arch, depth + 1, false);
                    } else {
                        let slot = if arch == Arch::X86 && ins.mn == "movq" {
                            rx!(r"^(-?0x[0-9a-f]+\(%rbp\)), %r\w+$")
                                .captures(&ins.ops)
                                .map(|c| c[1].to_owned())
                        } else if arch == Arch::Arm64 && ins.mn == "ldr" {
                            rx!(r"^x\d+, (\[(sp|x29), #-?0x[0-9a-f]+\])$")
                                .captures(&ins.ops)
                                .map(|c| c[1].to_owned())
                        } else {
                            None
                        };
                        if let Some(slot) = slot {
                            g = self.slot_pinned(k, &slot, arch, depth);
                        }
                    }
                }
                let g = g?;
                if sym.as_ref().is_some_and(|s| s != &g) {
                    return None;
                }
                sym = Some(g);
                found = true;
                break;
            }
            if found {
                continue;
            }
            if b == 0 {
                if ARG_REGS.contains(&reg) {
                    return None;
                }
                continue;
            }
            if self.pred[b].is_empty() {
                if self.padding(b) {
                    continue;
                }
                return None;
            }
            for &p in &self.pred[b] {
                if seen.insert(p) {
                    work.push((p, self.end(p)));
                }
            }
        }
        sym
    }
    fn slot_pinned(&self, i: usize, slot: &str, arch: Arch, depth: usize) -> Option<String> {
        let mut sym = None;
        let mut seen = HashSet::new();
        let mut work = vec![(self.block_of[i], i)];
        while let Some((b, upto)) = work.pop() {
            let mut found = false;
            for k in (self.starts[b]..upto).rev() {
                let ins = &self.insns[k];
                if !ins.ops.contains(slot) {
                    continue;
                }
                let src = if arch == Arch::X86 {
                    if ins.ops.rsplit(',').next().unwrap().trim() != slot {
                        continue;
                    }
                    if ins.mn == "movq" {
                        rx!(r"^%(r\w+), ")
                            .captures(&ins.ops)
                            .map(|c| x86_family(&c[1]))
                    } else {
                        None
                    }
                } else if ins.mn == "str" {
                    if !ins.ops.ends_with(slot) {
                        None
                    } else {
                        rx!(r"^x(\d+), ")
                            .captures(&ins.ops)
                            .map(|c| format!("x{}", &c[1]))
                    }
                } else if ins.mn.starts_with("st") {
                    None
                } else {
                    continue;
                };
                let g = self.pinned(k, &src?, arch, depth + 1, false)?;
                if sym.as_ref().is_some_and(|s| s != &g) {
                    return None;
                }
                sym = Some(g);
                found = true;
                break;
            }
            if found || b == 0 {
                continue;
            }
            if self.pred[b].is_empty() {
                if self.padding(b) {
                    continue;
                }
                return None;
            }
            for &p in &self.pred[b] {
                if seen.insert(p) {
                    work.push((p, self.end(p)));
                }
            }
        }
        sym
    }
}
fn idiom(insns: &[Insn], i: usize, arch: Arch, fmt: Format) -> Option<&'static str> {
    let ins = &insns[i];
    let window = &insns[i.saturating_sub(16)..i];
    let near = &window[window.len().saturating_sub(4)..];
    let os = || {
        insns
            .iter()
            .any(|w| w.relocs.iter().any(|(_, t)| t.contains("OS_FUNCTIONS")))
    };
    if arch == Arch::X86 {
        if let Some(c) = rx!(r"^\*0x17\(%(\w+)\)$").captures(&ins.ops) {
            let mut untag = near.iter().any(|w| {
                (w.mn.starts_with("lea") && w.ops.starts_with("-0x1(%"))
                    || (w.mn.starts_with("dec") && w.ops == "%rdi")
            });
            let tag = window
                .iter()
                .any(|w| w.mn.starts_with("and") && w.ops.starts_with("$0x3,"));
            if !untag && fmt == Format::Coff && window.len() >= 2 {
                let mov = &window[window.len() - 2];
                let dec = &window[window.len() - 1];
                untag = mov.mn.starts_with("mov")
                    && mov.ops == format!("%{}, %rcx", &c[1])
                    && dec.mn.starts_with("dec")
                    && dec.ops == "%rcx";
            }
            if untag && tag {
                return Some("io_error_drop");
            }
        }
        if rx!(r"^\*0x[0-9a-f]+\(%\w+\)$").is_match(&ins.ops) && os() {
            return Some("io_os_functions");
        }
    } else {
        let reg = ins.ops.split(',').next().unwrap().trim();
        for w in near {
            if !["ldur", "ldr"].contains(&w.mn.as_str()) {
                continue;
            }
            if let Some(c) = rx!(r"^(x\d+), \[(x\d+), #0x17\]$").captures(&w.ops) {
                if &c[1] == reg
                    && near
                        .iter()
                        .any(|v| v.mn == "sub" && v.ops.ends_with(&format!(", {}, #0x1", &c[2])))
                {
                    return Some("io_error_drop");
                }
            }
        }
        if ins.mn.starts_with("bl")
            && near.iter().any(|w| {
                w.mn == "ldr"
                    && rx!(r"^(x\d+), \[(x\d+), #0x[0-9a-f]+\]$")
                        .captures(&w.ops)
                        .is_some_and(|c| &c[1] == reg && &c[2] == reg)
            })
            && os()
        {
            return Some("io_os_functions");
        }
    }
    None
}
fn tlv_thunk(cfg: &Cfg<'_>, i: usize, reg: &str) -> bool {
    for k in (i.saturating_sub(7)..i).rev() {
        let w = &cfg.insns[k];
        if !writes(w, reg, Arch::Arm64) {
            continue;
        }
        if w.mn != "ldr" || cfg.block_of[k] != cfg.block_of[i] {
            return false;
        }
        if let Some(c) = rx!(r"^(x\d+), \[(x\d+)\]$").captures(&w.ops) {
            return &c[1] == reg && cfg.pinned(k, &c[2], Arch::Arm64, 0, true).is_some();
        }
        return false;
    }
    false
}
pub(crate) fn analyze_node(node: &mut Node, insns: &[Insn], arch: Arch) {
    if insns.is_empty() {
        return;
    }
    let switch_at: HashSet<_> = insns
        .iter()
        .enumerate()
        .filter(|(i, ins)| {
            ((arch == Arch::X86
                && ins.mn.starts_with("jmp")
                && ins.ops.starts_with('*')
                && !ins.ops.contains("(%rip)"))
                || (arch == Arch::Arm64 && ins.mn == "br"))
                && switch(insns, *i, arch)
        })
        .map(|(i, _)| i)
        .collect();
    node.switches = switch_at.len();
    let mut cfg = None;
    for (i, ins) in insns.iter().enumerate() {
        let mn = ins.mn.as_str();
        if arch == Arch::X86 {
            let call = mn.starts_with("call");
            if !call && !mn.starts_with('j') {
                continue;
            }
            if !ins.ops.starts_with('*') {
                for (_, t) in &ins.relocs {
                    node.calls.insert(node.fmt.norm(t));
                }
                continue;
            }
            let g = if ins.ops.contains("(%rip)") {
                got_symbol(ins, false)
            } else {
                rx!(r"^\*%(\w+)$").captures(&ins.ops).and_then(|c| {
                    cfg.get_or_insert_with(|| Cfg::new(insns, arch, &switch_at))
                        .pinned(i, &x86_family(&c[1]), arch, 0, false)
                })
            };
            if let Some(g) = g {
                node.calls.insert(node.fmt.norm(&g));
                continue;
            }
            if switch_at.contains(&i) {
                continue;
            }
            if let Some(id) = idiom(insns, i, arch, node.fmt) {
                node.idioms.insert(id.into());
                continue;
            }
            if call {
                node.indirect_calls += 1;
            } else {
                node.indirect_jmps += 1;
            }
        } else {
            if ["b", "bl"].contains(&mn) || starts(mn, &["b.", "cbz", "cbnz", "tbz", "tbnz"]) {
                for (_, t) in &ins.relocs {
                    node.calls.insert(node.fmt.norm(t));
                }
                continue;
            }
            if !["blr", "br"].contains(&mn) && !starts(mn, &["blra", "bra"]) {
                continue;
            }
            let reg = a64_family(ins.ops.split(',').next().unwrap().trim())
                .unwrap_or_else(|| ins.ops.clone());
            let cfg = cfg.get_or_insert_with(|| Cfg::new(insns, arch, &switch_at));
            if let Some(g) = cfg.pinned(i, &reg, arch, 0, false) {
                node.calls.insert(node.fmt.norm(&g));
                continue;
            }
            if mn.starts_with("bl") && tlv_thunk(cfg, i, &reg) {
                node.calls.insert("__tlv_get_addr".into());
                continue;
            }
            if switch_at.contains(&i) {
                continue;
            }
            if let Some(id) = idiom(insns, i, arch, node.fmt) {
                node.idioms.insert(id.into());
                continue;
            }
            if mn.starts_with("bl") {
                node.indirect_calls += 1;
            } else {
                node.indirect_jmps += 1;
            }
        }
        node.sites
            .push(format!("+{:#x} {} {}", ins.addr, ins.mn, ins.ops));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ins(addr: u64, mn: &str, ops: &str) -> Insn {
        Insn {
            addr,
            mn: mn.into(),
            ops: ops.into(),
            relocs: Vec::new(),
        }
    }
    #[test]
    fn got_call_clobbers_return_register() {
        let mut call = ins(0, "callq", "*0x0(%rip)");
        call.relocs
            .push(("R_X86_64_GOTPCREL".into(), "dlsym".into()));
        let later = ins(1, "callq", "*%rax");
        let body = [call, later.clone()];
        assert_eq!(
            Cfg::new(&body, Arch::X86, &HashSet::new()).pinned(1, "rax", Arch::X86, 0, false),
            None
        );
        let mut load = ins(0, "movq", "0x0(%rip), %rax");
        load.relocs
            .push(("R_X86_64_GOTPCREL".into(), "plugin_deactivate".into()));
        let body = [load, later];
        assert_eq!(
            Cfg::new(&body, Arch::X86, &HashSet::new())
                .pinned(1, "rax", Arch::X86, 0, false)
                .as_deref(),
            Some("plugin_deactivate")
        );
    }
    fn drop_site(fmt: Format, mov: &str, dec: &str, call: &str, tag: bool) -> Node {
        let mut body = vec![
            ("testq", "%rax, %rax"),
            ("je", "0x68"),
            ("movl", "%eax, %ecx"),
        ];
        if tag {
            body.push(("andl", "$0x3, %ecx"));
        }
        body.extend([
            ("leal", "-0x2(%rcx), %edx"),
            ("cmpl", "$0x2, %edx"),
            ("jb", "0x68"),
            ("testq", "%rcx, %rcx"),
            ("je", "0x68"),
        ]);
        body.push(mov.split_once(' ').unwrap());
        body.push(dec.split_once(' ').unwrap());
        body.extend([("callq", call), ("retq", "")]);
        let body: Vec<_> = body
            .into_iter()
            .enumerate()
            .map(|(k, (mn, ops))| ins(0x48 + 4 * k as u64, mn, ops))
            .collect();
        let mut n = Node::new("fixture.o".into(), ".text".into(), 0, fmt);
        analyze_node(&mut n, &body, Arch::X86);
        n
    }
    #[test]
    fn win64_io_drop_idiom_and_all_near_misses() {
        let n = drop_site(
            Format::Coff,
            "movq %rax, %rcx",
            "decq %rcx",
            "*0x17(%rax)",
            true,
        );
        assert_eq!(n.idioms, BTreeSet::from(["io_error_drop".into()]));
        assert_eq!(n.indirect_calls, 0);
        for n in [
            drop_site(
                Format::Elf,
                "movq %rax, %rcx",
                "decq %rcx",
                "*0x17(%rax)",
                true,
            ),
            drop_site(
                Format::Coff,
                "movq %rax, %rcx",
                "decq %rcx",
                "*0x17(%rax)",
                false,
            ),
            drop_site(
                Format::Coff,
                "movq %rbx, %rcx",
                "decq %rcx",
                "*0x17(%rax)",
                true,
            ),
            drop_site(
                Format::Coff,
                "movq %rax, %rdx",
                "decq %rdx",
                "*0x17(%rax)",
                true,
            ),
            drop_site(
                Format::Coff,
                "movq %rax, %rcx",
                "decq %rcx",
                "*0x1f(%rax)",
                true,
            ),
        ] {
            assert!(n.idioms.is_empty());
            assert_eq!(n.indirect_calls, 1);
        }
    }
    #[test]
    fn got_pinning_requires_every_cfg_predecessor() {
        for same in [true, false] {
            let mut first = ins(0, "movq", "0x0(%rip), %rax");
            first.relocs.push(("GOTPCREL".into(), "first".into()));
            let mut second = ins(2, "movq", "0x0(%rip), %rax");
            second.relocs.push((
                "GOTPCREL".into(),
                if same { "first" } else { "second" }.into(),
            ));
            let body = [first, ins(1, "je", "0x3"), second, ins(3, "callq", "*%rax")];
            let result =
                Cfg::new(&body, Arch::X86, &HashSet::new()).pinned(3, "rax", Arch::X86, 0, false);
            assert_eq!(result.as_deref(), same.then_some("first"));
        }
    }
    #[test]
    fn got_spill_reload_keeps_all_store_premises() {
        for overwrite in [false, true] {
            let mut load = ins(0, "movq", "0x0(%rip), %rax");
            load.relocs.push(("GOTPCREL".into(), "native_leaf".into()));
            let mut body = vec![
                load,
                ins(1, "movq", "%rax, -0x48(%rbp)"),
                ins(2, "callq", "0x0"),
            ];
            if overwrite {
                body.push(ins(3, "movq", "$0x0, -0x48(%rbp)"));
            }
            body.push(ins(4, "movq", "-0x48(%rbp), %r15"));
            body.push(ins(5, "callq", "*%r15"));
            let i = body.len() - 1;
            let result =
                Cfg::new(&body, Arch::X86, &HashSet::new()).pinned(i, "r15", Arch::X86, 0, false);
            assert_eq!(result.as_deref(), (!overwrite).then_some("native_leaf"));
        }
    }
    #[test]
    fn macho_tlv_descriptor_proof_rejects_clobber() {
        for clobber in [false, true] {
            let mut load = ins(0, "ldr", "x8, [x8]");
            load.relocs
                .push(("TLVP_LOAD_PAGEOFF12".into(), "_some_tls".into()));
            let mut body = vec![load];
            if clobber {
                body.push(ins(4, "mov", "x8, x0"));
            }
            body.push(ins(8, "ldr", "x9, [x8]"));
            body.push(ins(12, "blr", "x9"));
            let mut n = Node::new("fixture.o".into(), "__TEXT,__text".into(), 0, Format::MachO);
            analyze_node(&mut n, &body, Arch::Arm64);
            assert_eq!(n.indirect_calls, usize::from(clobber));
            assert_eq!(n.calls.contains("__tlv_get_addr"), !clobber);
        }
    }
}
