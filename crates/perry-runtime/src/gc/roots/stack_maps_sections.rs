//! Locating the compact GC-map section in the running image, per object file
//! format: Mach-O on Apple platforms, ELF on Linux, PE on Windows, and a stub
//! everywhere else.
//!
//! Its own file for the same reason `stack_maps_verify.rs` is: the parent is at
//! the 2000-line cap, and pure code motion is the cheapest way to stay under it.

#[cfg(target_os = "linux")]
use super::decode::{read_u16, read_u32, read_u64};
#[cfg(target_os = "linux")]
use std::ffi::c_void;

/// Every 64-bit Apple platform, not only macOS. iOS, iPadOS (which reports as
/// iOS), tvOS and visionOS are all aarch64 + Mach-O and share this loader
/// verbatim; gating it to `target_os = "macos"` sent them to the stub below,
/// where the section is never found and the index is empty — a collector with
/// no native roots, silently, on exactly the platforms that cannot be debugged
/// easily.
///
/// 64-bit only: watchOS's `arm64_32` has 32-bit pointers, while the map stores
/// function addresses as `u64` and this code does `usize` arithmetic on them.
/// The compiler refuses that target for the same reason.
#[cfg(target_vendor = "apple")]
pub(super) fn loaded_stack_map_sections() -> Result<Vec<&'static [u8]>, String> {
    use mach2::dyld::{_dyld_get_image_header, _dyld_get_image_vmaddr_slide, _dyld_image_count};

    const LC_SEGMENT_64: u32 = 0x19;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MachHeader64 {
        magic: u32,
        cpu_type: i32,
        cpu_subtype: i32,
        file_type: u32,
        command_count: u32,
        commands_size: u32,
        flags: u32,
        reserved: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct LoadCommand {
        command: u32,
        size: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SegmentCommand64 {
        command: u32,
        size: u32,
        segment_name: [u8; 16],
        vm_address: u64,
        vm_size: u64,
        file_offset: u64,
        file_size: u64,
        max_protection: i32,
        initial_protection: i32,
        section_count: u32,
        flags: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Section64 {
        section_name: [u8; 16],
        segment_name: [u8; 16],
        address: u64,
        size: u64,
        offset: u32,
        alignment: u32,
        relocation_offset: u32,
        relocation_count: u32,
        flags: u32,
        reserved1: u32,
        reserved2: u32,
        reserved3: u32,
    }

    fn fixed_name_matches(actual: &[u8; 16], expected: &[u8]) -> bool {
        actual.get(..expected.len()) == Some(expected)
            && actual.get(expected.len()).copied().unwrap_or(0) == 0
    }

    let mut sections = Vec::new();
    unsafe {
        for image_index in 0.._dyld_image_count() {
            let raw_header = _dyld_get_image_header(image_index);
            if raw_header.is_null() {
                continue;
            }
            let header = &*(raw_header.cast::<MachHeader64>());
            let slide = _dyld_get_image_vmaddr_slide(image_index);
            let mut command_ptr = raw_header
                .cast::<u8>()
                .add(std::mem::size_of::<MachHeader64>());
            for _ in 0..header.command_count {
                let load = std::ptr::read_unaligned(command_ptr.cast::<LoadCommand>());
                if load.size < std::mem::size_of::<LoadCommand>() as u32 {
                    break;
                }
                if load.command == LC_SEGMENT_64 {
                    let segment = std::ptr::read_unaligned(command_ptr.cast::<SegmentCommand64>());
                    let mut section_ptr = command_ptr.add(std::mem::size_of::<SegmentCommand64>());
                    for _ in 0..segment.section_count {
                        let section = std::ptr::read_unaligned(section_ptr.cast::<Section64>());
                        if fixed_name_matches(&section.segment_name, b"__PERRY_GCMAP")
                            && fixed_name_matches(&section.section_name, b"__perry_gcmap")
                        {
                            if let (Some(address), Ok(size)) = (
                                (section.address as isize).checked_add(slide),
                                usize::try_from(section.size),
                            ) {
                                if address > 0 && size != 0 {
                                    sections.push(std::slice::from_raw_parts(
                                        address as usize as *const u8,
                                        size,
                                    ));
                                }
                            }
                            break;
                        }
                        section_ptr = section_ptr.add(std::mem::size_of::<Section64>());
                    }
                }
                command_ptr = command_ptr.add(load.size as usize);
            }
        }
    }
    Ok(sections)
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux")))]
pub(super) fn loaded_stack_map_sections() -> Result<Vec<&'static [u8]>, String> {
    Ok(loaded_stack_map_section().into_iter().collect())
}

/// ELF (#7173, #8075): the `.perry_gcmap` sections of every loaded image.
///
/// Linker-provided `__start_`/`__stop_` symbols would need weak linkage
/// (unstable in Rust) or `-rdynamic` (not guaranteed), so instead: read
/// each `dl_iterate_phdr` image's ELF section headers for `.perry_gcmap`
/// (`sh_addr`, `sh_size`) and add that image's `dlpi_addr` load bias. The
/// executable has an empty `dlpi_name`, for which `/proc/self/exe` is the
/// stable path. Reading only that first image is unsound when the runtime is
/// a provider and generated code lives in an app dylib: its live native roots
/// disappear from the collector exactly when a full collection evacuates.
#[cfg(target_os = "linux")]
pub(super) fn loaded_stack_map_sections() -> Result<Vec<&'static [u8]>, String> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    #[repr(C)]
    struct DlPhdrInfo {
        dlpi_addr: usize,
        dlpi_name: *const std::os::raw::c_char,
        dlpi_phdr: *const ElfProgramHeader,
        dlpi_phnum: u16,
    }
    #[repr(C)]
    struct ElfProgramHeader {
        p_type: u32,
        _p_flags: u32,
        _p_offset: u64,
        p_vaddr: u64,
        _p_paddr: u64,
        _p_filesz: u64,
        p_memsz: u64,
        _p_align: u64,
    }
    struct SectionScan {
        sections: Vec<&'static [u8]>,
        unreadable_images: Vec<String>,
    }
    #[allow(clashing_extern_declarations)]
    unsafe extern "C" {
        fn dl_iterate_phdr(
            callback: unsafe extern "C" fn(*mut DlPhdrInfo, usize, *mut c_void) -> i32,
            data: *mut c_void,
        ) -> i32;
    }
    unsafe extern "C" fn collect(info: *mut DlPhdrInfo, _size: usize, data: *mut c_void) -> i32 {
        let Some(info) = info.as_ref() else {
            return 0;
        };
        let image_name = if info.dlpi_name.is_null() {
            &[][..]
        } else {
            CStr::from_ptr(info.dlpi_name).to_bytes()
        };
        // The kernel-provided vDSO has no backing file. It cannot contain
        // Perry-generated code, so it is the sole unreadable-image exception.
        if image_name == b"linux-vdso.so.1" || image_name == b"linux-gate.so.1" {
            return 0;
        }
        let path = if image_name.is_empty() {
            Path::new("/proc/self/exe")
        } else {
            Path::new(std::ffi::OsStr::from_bytes(image_name))
        };
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                let scan = &mut *data.cast::<SectionScan>();
                scan.unreadable_images
                    .push(format!("{} ({error})", path.display()));
                return 0;
            }
        };
        let Some((addr, size)) = elf_section_vaddr(&bytes, b".perry_gcmap") else {
            return 0;
        };
        let Some(start) = info.dlpi_addr.checked_add(addr) else {
            return 0;
        };
        let Some(section_end) = addr.checked_add(size) else {
            return 0;
        };
        const PT_LOAD: u32 = 1;
        let mapped = !info.dlpi_phdr.is_null()
            && std::slice::from_raw_parts(info.dlpi_phdr, usize::from(info.dlpi_phnum))
                .iter()
                .filter(|header| header.p_type == PT_LOAD)
                .any(|header| {
                    let Ok(segment_start) = usize::try_from(header.p_vaddr) else {
                        return false;
                    };
                    let Some(segment_end) = usize::try_from(header.p_memsz)
                        .ok()
                        .and_then(|size| segment_start.checked_add(size))
                    else {
                        return false;
                    };
                    addr >= segment_start && section_end <= segment_end
                });
        // The on-disk path can be replaced after dlopen. Validate its claimed
        // address against the loader's actual PT_LOAD ranges before turning
        // it into a slice, so a stale or hostile section table cannot make GC
        // initialization read outside the mapped image.
        if mapped && start != 0 && size != 0 {
            let scan = &mut *data.cast::<SectionScan>();
            scan.sections
                .push(std::slice::from_raw_parts(start as *const u8, size));
        }
        0
    }

    let mut scan = SectionScan {
        sections: Vec::new(),
        unreadable_images: Vec::new(),
    };
    unsafe {
        dl_iterate_phdr(collect, (&mut scan as *mut SectionScan).cast::<c_void>());
    }
    if scan.unreadable_images.is_empty() {
        Ok(scan.sections)
    } else {
        Err(format!(
            "unreadable loaded ELF image(s): {}",
            scan.unreadable_images.join(", ")
        ))
    }
}

/// Minimal ELF64 section-header walk: returns (sh_addr, sh_size) for the
/// named section. Same defensive read style as the stack-map parser.
#[cfg(target_os = "linux")]
fn elf_section_vaddr(bytes: &[u8], name: &[u8]) -> Option<(usize, usize)> {
    if bytes.get(..4)? != b"\x7fELF" || *bytes.get(4)? != 2 {
        return None; // not ELF64
    }
    let shoff = read_u64(bytes, 0x28)? as usize;
    let shentsize = read_u16(bytes, 0x3A)? as usize;
    let shnum = read_u16(bytes, 0x3C)? as usize;
    let shstrndx = read_u16(bytes, 0x3E)? as usize;
    let strtab_hdr = shoff.checked_add(shstrndx.checked_mul(shentsize)?)?;
    let strtab_off = read_u64(bytes, strtab_hdr.checked_add(0x18)?)? as usize;
    for i in 0..shnum {
        let hdr = shoff.checked_add(i.checked_mul(shentsize)?)?;
        let name_off = read_u32(bytes, hdr)? as usize;
        let name_pos = strtab_off.checked_add(name_off)?;
        let candidate = bytes.get(name_pos..name_pos.checked_add(name.len())?)?;
        let terminator = bytes.get(name_pos + name.len()).copied().unwrap_or(1);
        if candidate == name && terminator == 0 {
            // Only an SHF_ALLOC section has a runtime virtual address. Refuse
            // a file-only namesake before constructing a slice from sh_addr.
            const SHF_ALLOC: u64 = 0x2;
            if read_u64(bytes, hdr.checked_add(0x08)?)? & SHF_ALLOC == 0 {
                return None;
            }
            let addr = read_u64(bytes, hdr.checked_add(0x10)?)? as usize;
            let size = read_u64(bytes, hdr.checked_add(0x20)?)? as usize;
            return Some((addr, size));
        }
    }
    None
}

/// Windows/PE: the `.pgcmap` section of the running image.
///
/// The name is seven bytes because a PE image section header has an 8-byte name
/// field — `.perry_gcmap` would be truncated on the way into the image and the
/// lookup below could never match it. `gc_map::COFF_SECTION_NAME` is the
/// compiler-side half of that agreement.
///
/// `GetModuleHandleW(NULL)` returns the image base, which is also a valid
/// `IMAGE_DOS_HEADER`; the section table follows the optional header, whose
/// size the file header records rather than being fixed.
#[cfg(target_os = "windows")]
fn loaded_stack_map_section() -> Option<&'static [u8]> {
    const IMAGE_DOS_SIGNATURE: u16 = 0x5A4D; // "MZ"
    const IMAGE_NT_SIGNATURE: u32 = 0x0000_4550; // "PE\0\0"
    const SECTION_HEADER_SIZE: usize = 40;
    const SECTION_NAME: &[u8] = b".pgcmap";

    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut core::ffi::c_void;
    }

    unsafe {
        let base = GetModuleHandleW(std::ptr::null()) as *const u8;
        if base.is_null() {
            return None;
        }
        if std::ptr::read_unaligned(base as *const u16) != IMAGE_DOS_SIGNATURE {
            return None;
        }
        // e_lfanew sits at offset 0x3C of the DOS header.
        let nt_offset = std::ptr::read_unaligned(base.add(0x3C) as *const u32) as usize;
        let nt = base.add(nt_offset);
        if std::ptr::read_unaligned(nt as *const u32) != IMAGE_NT_SIGNATURE {
            return None;
        }
        // IMAGE_FILE_HEADER follows the 4-byte signature: NumberOfSections at
        // +2, SizeOfOptionalHeader at +16.
        let file_header = nt.add(4);
        let section_count = std::ptr::read_unaligned(file_header.add(2) as *const u16) as usize;
        let optional_size = std::ptr::read_unaligned(file_header.add(16) as *const u16) as usize;
        let sections = file_header.add(20).add(optional_size);

        for index in 0..section_count {
            let header = sections.add(index * SECTION_HEADER_SIZE);
            let name = std::slice::from_raw_parts(header, 8);
            // Names shorter than eight bytes are NUL-padded.
            let trimmed = match name.iter().position(|b| *b == 0) {
                Some(end) => &name[..end],
                None => name,
            };
            if trimmed != SECTION_NAME {
                continue;
            }
            let virtual_size = std::ptr::read_unaligned(header.add(8) as *const u32) as usize;
            let virtual_address = std::ptr::read_unaligned(header.add(12) as *const u32) as usize;
            if virtual_size == 0 || virtual_address == 0 {
                return None;
            }
            return Some(std::slice::from_raw_parts(
                base.add(virtual_address),
                virtual_size,
            ));
        }
    }
    None
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "windows")))]
fn loaded_stack_map_section() -> Option<&'static [u8]> {
    None
}
