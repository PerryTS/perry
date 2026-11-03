//! Read archives directly; only instruction decoding crosses the LLVM C API.
use crate::graph::{temp_label, Graph, Node};
use crate::instructions::{analyze_node, Arch, Format, Insn};
use anyhow::{bail, Context, Result};
use llvm_sys::{disassembler::*, target::*};
use object::{Object, ObjectSection, ObjectSymbol, RelocationFlags, RelocationTarget, SectionKind};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{CStr, CString},
    path::PathBuf,
    sync::Once,
};

struct Disassembler {
    ctx: LLVMDisasmContextRef,
    branch: Box<std::cell::Cell<Option<u64>>>,
}
extern "C" fn branch_target(
    info: *mut std::ffi::c_void,
    value: u64,
    reference_type: *mut u64,
    _pc: u64,
    _name: *mut *const std::ffi::c_char,
) -> *const std::ffi::c_char {
    // MC's symbolizer supplies the decoded absolute target before printing
    // the PC-relative operand. The C printer otherwise prints the displacement,
    // unlike llvm-objdump. Retain it for the same CFG addresses as objdump.
    unsafe {
        if *reference_type == LLVMDisassembler_ReferenceType_In_Branch {
            (*(info as *const std::cell::Cell<Option<u64>>)).set(Some(value));
        }
        *reference_type = LLVMDisassembler_ReferenceType_InOut_None;
        if !_name.is_null() {
            *_name = std::ptr::null();
        }
    }
    std::ptr::null()
}
impl Disassembler {
    fn new(arch: Arch) -> Result<Self> {
        static INIT: Once = Once::new();
        // LLVM target registration is process-global, and is run once before
        // creating contexts. Each context is confined to its owning thread.
        INIT.call_once(|| unsafe {
            LLVMInitializeX86TargetInfo();
            LLVMInitializeX86TargetMC();
            LLVMInitializeX86Disassembler();
            LLVMInitializeX86AsmPrinter();
            LLVMInitializeAArch64TargetInfo();
            LLVMInitializeAArch64TargetMC();
            LLVMInitializeAArch64Disassembler();
            LLVMInitializeAArch64AsmPrinter();
        });
        let triple = CString::new(if arch == Arch::X86 {
            "x86_64-unknown-linux-gnu"
        } else {
            "aarch64-unknown-linux-gnu"
        })?;
        let branch = Box::new(std::cell::Cell::new(None::<u64>));
        let info = branch.as_ref() as *const _ as *mut std::ffi::c_void;
        let ctx = unsafe { LLVMCreateDisasm(triple.as_ptr(), info, 0, None, Some(branch_target)) };
        if ctx.is_null() {
            bail!("LLVM 22 cannot create disassembler");
        }
        unsafe {
            LLVMSetDisasmOptions(ctx, LLVMDisassembler_Option_PrintImmHex);
        }
        Ok(Self { ctx, branch })
    }
    fn instruction(&self, bytes: &[u8], addr: u64) -> (usize, Insn) {
        self.branch.set(None);
        let mut out: [std::ffi::c_char; 1024] = [0; 1024];
        // LLVM does not write to Bytes (the C API predates const correctness).
        // The output is NUL-terminated in the fixed-size buffer.
        let len = unsafe {
            LLVMDisasmInstruction(
                self.ctx,
                bytes.as_ptr() as *mut u8,
                bytes.len() as u64,
                addr,
                out.as_mut_ptr(),
                out.len(),
            )
        };
        let text = unsafe { CStr::from_ptr(out.as_ptr()) }.to_string_lossy();
        let mut ins = Insn::new(addr, &text);
        if let Some(target) = self.branch.get() {
            if let Some((head, _)) = ins.ops.rsplit_once(',') {
                ins.ops = format!("{head}, {target:#x}");
            } else {
                ins.ops = format!("{target:#x}");
            }
        }
        (len, ins)
    }
}
impl Drop for Disassembler {
    fn drop(&mut self) {
        unsafe {
            LLVMDisasmDispose(self.ctx);
        }
    }
}

fn reloc_type(flags: RelocationFlags, arch: Arch) -> String {
    // Relocation numbers belong to an architecture as well as a file format.
    // Only these categories distinguish data from GOT/TLVP pointer loads in
    // the compatibility proof; every other relocation still names its target.
    match (flags, arch) {
        (RelocationFlags::Elf { r_type }, Arch::X86) => match r_type {
            object::elf::R_X86_64_GOTPCREL => "GOTPCREL",
            object::elf::R_X86_64_GOTPCRELX => "GOTPCRELX",
            object::elf::R_X86_64_REX_GOTPCRELX => "REX_GOTPCRELX",
            _ => "other",
        },
        (RelocationFlags::Elf { r_type }, Arch::Arm64) => match r_type {
            object::elf::R_AARCH64_ADR_GOT_PAGE => "ADR_GOT_PAGE",
            object::elf::R_AARCH64_LD64_GOT_LO12_NC => "LD64_GOT_LO12_NC",
            _ => "other",
        },
        (
            RelocationFlags::MachO {
                r_type: object::macho::X86_64_RELOC_GOT_LOAD,
                ..
            },
            Arch::X86,
        ) => "GOT_LOAD",
        (RelocationFlags::MachO { r_type, .. }, Arch::Arm64) => match r_type {
            object::macho::ARM64_RELOC_GOT_LOAD_PAGE21 => "GOT_LOAD_PAGE21",
            object::macho::ARM64_RELOC_GOT_LOAD_PAGEOFF12 => "GOT_LOAD_PAGEOFF12",
            object::macho::ARM64_RELOC_TLVP_LOAD_PAGE21 => "TLVP_LOAD_PAGE21",
            object::macho::ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => "TLVP_LOAD_PAGEOFF12",
            _ => "other",
        },
        _ => "other",
    }
    .into()
}

struct SectionTask<'a> {
    obj: std::sync::Arc<str>,
    sec: std::sync::Arc<str>,
    base: u64,
    data: &'a [u8],
    arch: Arch,
    fmt: Format,
    labels: BTreeMap<u64, Vec<String>>,
    relocs: Vec<(u64, String, String)>,
}
fn parse_section(task: SectionTask<'_>, dis: &Disassembler) -> Vec<Node> {
    let SectionTask {
        obj: name,
        sec,
        base,
        data,
        arch,
        fmt,
        mut labels,
        relocs,
    } = task;
    let mut nodes = Vec::new();
    let mut ri = 0;
    let mut off = 0;
    let mut insns = Vec::new();
    let mut current: Option<Node> = None;
    while off < data.len() {
        let addr = base + off as u64;
        if let Some(names) = labels.remove(&addr) {
            if let Some(n) = current
                .as_mut()
                .filter(|_| names.iter().all(|n| temp_label(n)))
            {
                n.names.extend(names.into_iter().map(|n| fmt.norm_owned(n)));
            } else {
                if let Some(mut n) = current.take() {
                    analyze_node(&mut n, &insns, arch);
                    nodes.push(n);
                }
                insns.clear();
                let mut n = Node::new(name.clone(), sec.clone(), addr, fmt);
                n.names = names.into_iter().map(|n| fmt.norm_owned(n)).collect();
                current = Some(n);
            }
        }
        let (len, mut ins) = dis.instruction(&data[off..], addr);
        let len = if len == 0 {
            if arch == Arch::Arm64 {
                4.min(data.len() - off)
            } else {
                1
            }
        } else {
            len
        };
        while ri < relocs.len() && relocs[ri].0 < addr {
            ri += 1;
        }
        while ri < relocs.len() && relocs[ri].0 < addr + len as u64 {
            ins.relocs
                .push((relocs[ri].1.clone(), relocs[ri].2.clone()));
            ri += 1;
        }
        if current.is_some() {
            insns.push(ins);
        }
        off += len;
    }
    // Symbols exactly at the section end have no disassembled label in
    // objdump (e.g. assembly unwind-range end markers). They are not nodes.
    for (addr, names) in labels {
        if addr >= base + data.len() as u64 {
            continue;
        }
        if let Some(n) = current
            .as_mut()
            .filter(|_| names.iter().all(|n| temp_label(n)))
        {
            n.names.extend(names.into_iter().map(|n| fmt.norm_owned(n)));
        } else {
            if let Some(mut n) = current.take() {
                analyze_node(&mut n, &insns, arch);
                nodes.push(n);
            }
            insns.clear();
            let mut n = Node::new(name.clone(), sec.clone(), addr, fmt);
            n.names = names.into_iter().map(|n| fmt.norm_owned(n)).collect();
            current = Some(n);
        }
    }
    if let Some(mut n) = current {
        analyze_node(&mut n, &insns, arch);
        nodes.push(n);
    }
    nodes
}
/// Decode independent sections on a bounded worker pool. Results are restored
/// to archive/section order before graph resolution, so first-definition and
/// witness ordering remain exactly the Python analyzer's.
fn decode_sections<'data>(
    produce: impl FnOnce(&std::sync::mpsc::SyncSender<Vec<(usize, SectionTask<'data>)>>) -> Result<()>,
) -> Result<Vec<Node>> {
    // Keep only a bounded number of sections' relocation strings alive while
    // the producer reads the archive. Large one-CGU objects otherwise require
    // the entire relocation listing alongside the completed graph.
    let (sender, receiver) = std::sync::mpsc::sync_channel::<Vec<(usize, SectionTask<'data>)>>(4);
    let queue = std::sync::Mutex::new(receiver);
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(8);
    let parts = std::thread::scope(|scope| -> Result<_> {
        let mut handles = Vec::new();
        for _ in 0..workers {
            let queue = &queue;
            handles.push(scope.spawn(move || -> Result<Vec<(usize, Vec<Node>)>> {
                let mut x86 = None;
                let mut arm = None;
                let mut out = Vec::new();
                let mut failed = None;
                loop {
                    let Ok(batch) = queue.lock().expect("section queue").recv() else {
                        break;
                    };
                    for (index, task) in batch {
                        // Drain on an initialization error so the producer cannot
                        // remain blocked on a full channel.
                        if failed.is_some() {
                            continue;
                        }
                        let slot = if task.arch == Arch::X86 {
                            &mut x86
                        } else {
                            &mut arm
                        };
                        if slot.is_none() {
                            match Disassembler::new(task.arch) {
                                Ok(dis) => *slot = Some(dis),
                                Err(e) => {
                                    failed = Some(e);
                                    continue;
                                }
                            }
                        }
                        out.push((index, parse_section(task, slot.as_ref().unwrap())));
                    }
                }
                if let Some(e) = failed {
                    Err(e)
                } else {
                    Ok(out)
                }
            }));
        }
        let produced = produce(&sender);
        drop(sender);
        let decoded = handles
            .into_iter()
            .map(|h| h.join().expect("archive decoder thread"))
            .collect::<Result<Vec<_>>>();
        produced?;
        decoded
    })?;
    let mut parts: Vec<_> = parts.into_iter().flatten().collect();
    parts.sort_unstable_by_key(|(i, _)| *i);
    Ok(parts.into_iter().flat_map(|(_, ns)| ns).collect())
}

pub fn build_graph(archives: &[PathBuf]) -> Result<Graph> {
    let mut nodes = Vec::new();
    let mut globals = HashSet::new();
    for archive in archives {
        let bytes =
            std::fs::read(archive).with_context(|| format!("read {}", archive.display()))?;
        let ar = object::read::archive::ArchiveFile::parse(bytes.as_slice())
            .with_context(|| format!("parse archive {}", archive.display()))?;
        let decoded = decode_sections(|sender| {
            let mut index = 0;
            let mut batch = Vec::with_capacity(32);
            for member in ar.members() {
                let member = member?;
                let name: std::sync::Arc<str> =
                    String::from_utf8_lossy(member.name()).as_ref().into();
                let data = member.data(bytes.as_slice())?;
                if name.ends_with(".rmeta") || data.is_empty() {
                    continue;
                }
                if object::FileKind::parse(data) == Ok(object::FileKind::CoffImport) {
                    continue;
                }
                let file = object::File::parse(data)
                    .with_context(|| format!("{}({name})", archive.display()))?;
                let arch = match file.architecture() {
                    object::Architecture::X86_64 => Arch::X86,
                    object::Architecture::Aarch64 => Arch::Arm64,
                    a => bail!("unsupported architecture {a:?} in {name}"),
                };
                let fmt = match file.format() {
                    object::BinaryFormat::Elf => Format::Elf,
                    object::BinaryFormat::MachO => Format::MachO,
                    object::BinaryFormat::Coff => Format::Coff,
                    f => bail!("unsupported object format {f:?}"),
                };
                // Index once: Rust's one-function-per-section objects otherwise
                // turn a section-by-symbol scan into quadratic archive parsing.
                let mut symbols: HashMap<object::SectionIndex, BTreeMap<u64, Vec<String>>> =
                    HashMap::new();
                for symbol in file.symbols() {
                    let sym = symbol.name()?;
                    if !symbol.is_undefined() && (symbol.is_global() || symbol.is_weak()) {
                        globals.insert((name.clone(), fmt.norm(sym)));
                    }
                    let Some(section) = symbol.section_index() else {
                        continue;
                    };
                    if sym.is_empty()
                        || symbol.kind() == object::SymbolKind::Section && fmt != Format::Coff
                    {
                        continue;
                    }
                    if sym == "$d"
                        || sym.starts_with("$d.")
                        || sym == "$x"
                        || sym.starts_with("$x.")
                    {
                        continue;
                    }
                    symbols
                        .entry(section)
                        .or_default()
                        .entry(symbol.address())
                        .or_default()
                        .push(sym.to_owned());
                }
                for section in file.sections() {
                    if section.kind() != SectionKind::Text {
                        continue;
                    }
                    let data = section.data()?;
                    if data.is_empty() {
                        continue;
                    }
                    let sec = if fmt == Format::MachO {
                        format!(
                            "{},{}",
                            section.segment_name()?.unwrap_or(""),
                            section.name()?
                        )
                    } else {
                        section.name()?.to_owned()
                    };
                    let base = section.address();
                    let mut labels = symbols.remove(&section.index()).unwrap_or_default();
                    if labels.is_empty() {
                        labels.insert(base, vec![section.name()?.into()]);
                    }
                    let mut relocs = Vec::new();
                    for (offset, reloc) in section.relocations() {
                        let target = match reloc.target() {
                            RelocationTarget::Symbol(i) => {
                                let s = file.symbol_by_index(i)?;
                                if s.kind() == object::SymbolKind::Section {
                                    s.section_index()
                                        .and_then(|i| file.section_by_index(i).ok())
                                        .and_then(|s| s.name().ok().map(str::to_owned))
                                        .unwrap_or_else(|| s.name().unwrap_or("").to_owned())
                                } else {
                                    s.name()?.to_owned()
                                }
                            }
                            RelocationTarget::Section(i) => {
                                file.section_by_index(i)?.name()?.to_owned()
                            }
                            _ => continue,
                        };
                        relocs.push((base + offset, reloc_type(reloc.flags(), arch), target));
                    }
                    relocs.sort_by_key(|r| r.0);
                    batch.push((
                        index,
                        SectionTask {
                            obj: name.clone(),
                            sec: sec.into(),
                            base,
                            data,
                            arch,
                            fmt,
                            labels,
                            relocs,
                        },
                    ));
                    index += 1;
                    if batch.len() == 32 {
                        sender
                            .send(std::mem::replace(&mut batch, Vec::with_capacity(32)))
                            .map_err(|_| anyhow::anyhow!("archive decoder stopped"))?;
                    }
                }
            }
            if !batch.is_empty() {
                sender
                    .send(batch)
                    .map_err(|_| anyhow::anyhow!("archive decoder stopped"))?;
            }
            Ok(())
        })?;
        nodes.extend(decoded);
    }
    Graph::new(nodes, globals)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn llvm_branch_operands() {
        let d = Disassembler::new(Arch::X86).unwrap();
        let (_, branch) = d.instruction(&[0x75, 0x10], 16);
        assert_eq!(branch.ops, "0x22");
        let (_, load) = d.instruction(&[0x48, 0x8b, 0x05, 0, 0, 0, 0], 32);
        assert_eq!(load.ops, "(%rip), %rax");
        let (_, indirect) = d.instruction(&[0xff, 0xd0], 48);
        assert_eq!(indirect.ops, "*%rax");
        let a = Disassembler::new(Arch::Arm64).unwrap();
        let (_, branch) = a.instruction(&[0x04, 0x00, 0x00, 0x14], 32);
        assert_eq!(branch.ops, "0x30");
    }
    #[test]
    fn section_end_symbols_are_not_disassembled_nodes() {
        let task = SectionTask {
            obj: "fixture.o".into(),
            sec: ".text".into(),
            base: 0,
            data: &[0xc3],
            arch: Arch::X86,
            fmt: Format::Coff,
            labels: BTreeMap::from([(0, vec!["body".into()]), (1, vec!["range_end".into()])]),
            relocs: Vec::new(),
        };
        let nodes = parse_section(task, &Disassembler::new(Arch::X86).unwrap());
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].names, ["body"]);
    }
    #[test]
    fn relocation_numbers_are_architecture_specific() {
        let flags = RelocationFlags::MachO {
            r_type: object::macho::X86_64_RELOC_SUBTRACTOR,
            r_pcrel: false,
            r_length: 3,
        };
        assert_eq!(reloc_type(flags, Arch::X86), "other");
        assert_eq!(reloc_type(flags, Arch::Arm64), "GOT_LOAD_PAGE21");
        let flags = RelocationFlags::MachO {
            r_type: object::macho::X86_64_RELOC_GOT_LOAD,
            r_pcrel: true,
            r_length: 2,
        };
        assert_eq!(reloc_type(flags, Arch::X86), "GOT_LOAD");
        assert_eq!(reloc_type(flags, Arch::Arm64), "other");
    }
}
