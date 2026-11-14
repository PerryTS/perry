//! Perry: mimalloc's libc calls through the GOT, not the PLT (x86_64 glibc
//! Linux only; see perry_noplt_libc.h and ../VENDORED.md).
//!
//! Rust code binds these functions through the GOT at start. A PLT call of the
//! same function gets, under lld, a second JUMP_SLOT that ld.so binds lazily
//! (about 400 instructions on the first call each; chess +6K per run). Perry
//! links native x86_64 Linux executables with lld by default, so the build
//! fails if a single one of these calls is still a PLT call.

use std::convert::TryInto;
use std::path::{Path, PathBuf};

/// The libc functions mimalloc calls that Rust code also references through
/// the GOT (the JUMP_SLOTs lld adds over GNU ld in a chess link).
pub const GOT_FUNCTIONS: &[&str] = &[
    "__errno_location",
    "clock_gettime",
    "getrusage",
    "madvise",
    "memcpy",
    "memset",
    "mmap",
    "mprotect",
    "munmap",
    "prctl",
    "pthread_key_create",
    "pthread_key_delete",
    "pthread_setspecific",
    "syscall",
    "sysconf",
    "sysinfo",
];

/// Adds the shim to mimalloc's own compile. `true` when it applies (the build
/// then checks the object, [`verify`]).
pub fn configure(build: &mut cc::Build, compiler: &cc::Tool, manifest_dir: &str) -> bool {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    if var("CARGO_CFG_TARGET_ARCH") != "x86_64"
        || var("CARGO_CFG_TARGET_OS") != "linux"
        || var("CARGO_CFG_TARGET_ENV") != "gnu"
    {
        return false;
    }
    let header = Path::new(manifest_dir)
        .join("perry")
        .join("perry_noplt_libc.h");
    println!("cargo:rerun-if-changed={}", header.display());
    println!("cargo:rerun-if-changed=perry/noplt.rs");
    if compiler.is_like_clang() {
        // clang has no `noplt` attribute: the whole unit without a PLT.
        build.flag("-fno-plt");
    } else {
        build
            .flag("-include")
            .flag(header.to_str().expect("UTF-8 path"));
    }
    true
}

/// Fails the build when mimalloc's object still calls one of
/// [`GOT_FUNCTIONS`] through the PLT (`R_X86_64_PLT32` against it).
pub fn verify(out_dir: &Path) {
    let mut objects = Vec::new();
    collect_objects(out_dir, &mut objects);
    let objects: Vec<PathBuf> = objects
        .into_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("static.o"))
        })
        .collect();
    assert!(
        !objects.is_empty(),
        "perry: mimalloc's object (static.o) not found under {}",
        out_dir.display()
    );
    let mut plt = Vec::new();
    for object in &objects {
        let bytes = std::fs::read(object).expect("read mimalloc object");
        for name in plt32_targets(&bytes)
            .unwrap_or_else(|| panic!("perry: {} is not an ELF64 object", object.display()))
        {
            if GOT_FUNCTIONS.contains(&name.as_str()) && !plt.contains(&name) {
                plt.push(name);
            }
        }
    }
    assert!(
        plt.is_empty(),
        "perry: mimalloc still calls {:?} through the PLT; under lld each \
         gets a second, lazily bound slot (perry/perry_noplt_libc.h did not \
         apply: check CFLAGS for -fno-builtin / a compiler without asm labels)",
        plt
    );
}

fn collect_objects(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_objects(&path, out);
        } else if path.extension().is_some_and(|e| e == "o") {
            out.push(path);
        }
    }
}

/// The symbol names `R_X86_64_PLT32` relocations of a relocatable ELF64
/// little-endian object point at.
pub fn plt32_targets(elf: &[u8]) -> Option<Vec<String>> {
    const SHT_SYMTAB: u32 = 2;
    const SHT_RELA: u32 = 4;
    const R_X86_64_PLT32: u64 = 4;
    let u16_at = |o: usize| Some(u16::from_le_bytes(elf.get(o..o + 2)?.try_into().ok()?));
    let u32_at = |o: usize| Some(u32::from_le_bytes(elf.get(o..o + 4)?.try_into().ok()?));
    let u64_at = |o: usize| Some(u64::from_le_bytes(elf.get(o..o + 8)?.try_into().ok()?));
    if elf.get(..6)? != b"\x7fELF\x02\x01" {
        return None;
    }
    let shoff = u64_at(0x28)? as usize;
    let shentsize = u16_at(0x3a)? as usize;
    let shnum = u16_at(0x3c)? as usize;
    let section = |i: usize| {
        let base = shoff + i * shentsize;
        Some((
            u32_at(base + 4)?,             // sh_type
            u64_at(base + 0x18)? as usize, // sh_offset
            u64_at(base + 0x20)? as usize, // sh_size
            u32_at(base + 0x28)? as usize, // sh_link
        ))
    };
    let c_str = |o: usize| {
        let rest = elf.get(o..)?;
        let end = rest.iter().position(|&b| b == 0)?;
        Some(String::from_utf8_lossy(&rest[..end]).into_owned())
    };
    let mut names = Vec::new();
    for i in 0..shnum {
        let (kind, offset, size, link) = section(i)?;
        if kind != SHT_RELA {
            continue;
        }
        let (symtab_kind, symtab, _, strtab_index) = section(link)?;
        if symtab_kind != SHT_SYMTAB {
            continue;
        }
        let (_, strtab, _, _) = section(strtab_index)?;
        for entry in (offset..offset + size).step_by(24) {
            let info = u64_at(entry + 8)?;
            if info & 0xffff_ffff != R_X86_64_PLT32 {
                continue;
            }
            let symbol = symtab + (info >> 32) as usize * 24;
            names.push(c_str(strtab + u32_at(symbol)? as usize)?);
        }
    }
    Some(names)
}
