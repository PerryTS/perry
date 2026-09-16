//! The GCC-format LSDA walk, shared by every target's personality routine.
//!
//! Split out of `eh.rs` when windows-msvc stopped using funclet EH (#7354).
//! LLVM emits the *same* `GCC_except_table` for Perry's own personality on
//! COFF as it does on ELF/Mach-O — the table format follows the personality,
//! not the object format — so the Itanium personality in `eh.rs` and the
//! SEH-dispatched one in `eh_windows.rs` decode byte-identical data and must
//! share one decoder. Only the surrounding ABI differs: how the unwinder hands
//! over the LSDA pointer, the IP and the function start, and how control is
//! transferred once a pad is found.
//!
//! Ported from Rust std's `sys::personality::dwarf` (MIT OR Apache-2.0),
//! trimmed to the encodings LLVM emits for Perry's targets and with the
//! type-table/filter logic dropped (Perry landing pads are always
//! `catch ptr null` — catch-all; there are no cleanups and no filters in
//! generated code).

#![allow(non_upper_case_globals)]

/// The GCC-style LSDA layout: header (landing-pad base encoding + optional
/// base, type-table encoding + optional offset, call-site encoding), then the
/// call-site table sorted by start offset. Perry generates only catch-all
/// handlers, so the action/type tables need no interpretation: any non-zero
/// landing-pad offset is a handler.
///
/// `ip` must already be biased into the call instruction (the unwinder hands
/// back a return address, which can land in the *next* call-site range).
/// Callers own that adjustment because they own the ABI that produced the IP.
///
/// # Safety
/// `lsda` must point at a well-formed GCC exception table.
pub(crate) unsafe fn find_landing_pad_in_lsda(
    lsda: *const u8,
    ip: usize,
    func_start: usize,
) -> Result<Option<usize>, ()> {
    let mut reader = DwarfReader::new(lsda);

    let start_encoding = reader.read_u8();
    let lpad_base = if start_encoding != DW_EH_PE_omit {
        read_encoded_pointer(&mut reader, start_encoding, func_start)?
    } else {
        func_start
    };

    let ttype_encoding = reader.read_u8();
    if ttype_encoding != DW_EH_PE_omit {
        // Class-info offset — skipped, we never inspect the type table.
        reader.read_uleb128();
    }

    let call_site_encoding = reader.read_u8();
    let call_site_table_length = reader.read_uleb128();
    let action_table = reader.ptr.add(call_site_table_length as usize);

    while reader.ptr < action_table {
        let cs_start = read_encoded_offset(&mut reader, call_site_encoding)?;
        let cs_len = read_encoded_offset(&mut reader, call_site_encoding)?;
        let cs_lpad = read_encoded_offset(&mut reader, call_site_encoding)?;
        let _cs_action = reader.read_uleb128();
        // Sorted by cs_start: once past the ip, stop.
        if ip < func_start.wrapping_add(cs_start) {
            break;
        }
        if ip < func_start.wrapping_add(cs_start + cs_len) {
            return Ok(if cs_lpad == 0 {
                None
            } else {
                Some(lpad_base.wrapping_add(cs_lpad))
            });
        }
    }
    // IP not in the table: a non-invoke call site — no handler in this frame.
    Ok(None)
}

// ---------------------------------------------------------------------------
// DWARF exception-header encoded values (LSB spec, "dwarfext").
// ---------------------------------------------------------------------------

const DW_EH_PE_omit: u8 = 0xFF;
const DW_EH_PE_absptr: u8 = 0x00;
const DW_EH_PE_uleb128: u8 = 0x01;
const DW_EH_PE_udata2: u8 = 0x02;
const DW_EH_PE_udata4: u8 = 0x03;
const DW_EH_PE_udata8: u8 = 0x04;
const DW_EH_PE_sleb128: u8 = 0x09;
const DW_EH_PE_sdata2: u8 = 0x0A;
const DW_EH_PE_sdata4: u8 = 0x0B;
const DW_EH_PE_sdata8: u8 = 0x0C;
const DW_EH_PE_pcrel: u8 = 0x10;
const DW_EH_PE_indirect: u8 = 0x80;

struct DwarfReader {
    ptr: *const u8,
}

impl DwarfReader {
    fn new(ptr: *const u8) -> Self {
        DwarfReader { ptr }
    }

    unsafe fn read_u8(&mut self) -> u8 {
        let v = *self.ptr;
        self.ptr = self.ptr.add(1);
        v
    }

    unsafe fn read_unaligned<T: Copy>(&mut self) -> T {
        let v = (self.ptr as *const T).read_unaligned();
        self.ptr = self.ptr.add(core::mem::size_of::<T>());
        v
    }

    unsafe fn read_uleb128(&mut self) -> u64 {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            let byte = self.read_u8();
            result |= u64::from(byte & 0x7F) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return result;
            }
        }
    }

    unsafe fn read_sleb128(&mut self) -> i64 {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            let byte = self.read_u8();
            result |= u64::from(byte & 0x7F) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                // Sign-extend.
                if shift < 64 && byte & 0x40 != 0 {
                    result |= u64::MAX << shift;
                }
                return result as i64;
            }
        }
    }
}

/// Offset with a value-format-only encoding (application part must be zero —
/// LLVM uses these for the call-site table).
unsafe fn read_encoded_offset(reader: &mut DwarfReader, encoding: u8) -> Result<usize, ()> {
    if encoding == DW_EH_PE_omit || encoding & 0xF0 != 0 {
        return Err(());
    }
    Ok(match encoding & 0x0F {
        // LLVM uses absptr for offsets as well as pointers.
        DW_EH_PE_absptr => reader.read_unaligned::<usize>(),
        DW_EH_PE_uleb128 => reader.read_uleb128() as usize,
        DW_EH_PE_udata2 => reader.read_unaligned::<u16>() as usize,
        DW_EH_PE_udata4 => reader.read_unaligned::<u32>() as usize,
        DW_EH_PE_udata8 => reader.read_unaligned::<u64>() as usize,
        DW_EH_PE_sleb128 => reader.read_sleb128() as usize,
        DW_EH_PE_sdata2 => reader.read_unaligned::<i16>() as usize,
        DW_EH_PE_sdata4 => reader.read_unaligned::<i32>() as usize,
        DW_EH_PE_sdata8 => reader.read_unaligned::<i64>() as usize,
        _ => return Err(()),
    })
}

/// Pointer with an application part. Perry LSDAs use absptr or pcrel (the
/// encodings LLVM emits for the landing-pad base on Mach-O, ELF and COFF);
/// textrel/datarel/funcrel/aligned never appear and are rejected.
unsafe fn read_encoded_pointer(
    reader: &mut DwarfReader,
    encoding: u8,
    _func_start: usize,
) -> Result<usize, ()> {
    if encoding == DW_EH_PE_omit {
        return Err(());
    }
    let base: usize = match encoding & 0x70 {
        DW_EH_PE_absptr => 0,
        // Relative to the address of the encoded value itself.
        DW_EH_PE_pcrel => reader.ptr as usize,
        _ => return Err(()),
    };
    let mut ptr = if base == 0 {
        if encoding & 0x0F != DW_EH_PE_absptr {
            return Err(());
        }
        reader.read_unaligned::<usize>()
    } else {
        base.wrapping_add(read_encoded_offset(reader, encoding & 0x0F)?)
    };
    if encoding & DW_EH_PE_indirect != 0 {
        ptr = *(ptr as *const usize);
    }
    Ok(ptr)
}
