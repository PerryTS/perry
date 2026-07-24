#![cfg(target_os = "windows")]

pub mod app;
pub mod audio;
pub mod audio_playback;
pub mod deeplinks_stub;
#[cfg(target_os = "windows")]
pub mod dpi_compat;
pub mod drag_drop;
#[cfg(target_os = "windows")]
pub mod dwm;
pub mod issue_552_stub;
#[cfg(target_os = "windows")]
pub mod keyboard;
pub mod media_playback;
pub mod network_stub;
pub mod pointer;

// Install a vectored exception handler that prints crash info to stderr.
#[cfg(target_os = "windows")]
mod crash_handler {
    #[repr(C)]
    struct ExceptionRecord {
        exception_code: u32,
        exception_flags: u32,
        exception_record: *mut ExceptionRecord,
        exception_address: *mut core::ffi::c_void,
        number_parameters: u32,
        exception_information: [usize; 15],
    }

    // Accurate x64 Windows CONTEXT layout for the two fields we need.
    // (The previous `_padding:[u8;0x78]; Rip` mislabeled offset 0x78 — that
    // is Rax on x64; Rip is at 0xF8 — but nothing read it, so the bug was
    // dormant. We need Rsp@0x98 to recover the call chain.)
    #[repr(C)]
    #[allow(non_snake_case)]
    struct Context {
        _pad0: [u8; 0x98],              // → Rsp at 0x98
        Rsp: u64,                       // 0x98
        _pad1: [u8; 0xF8 - (0x98 + 8)], // 0xA0..0xF8
        Rip: u64,                       // 0xF8
    }

    #[repr(C)]
    struct ExceptionPointers {
        exception_record: *mut ExceptionRecord,
        context_record: *mut Context,
    }

    extern "system" {
        fn AddVectoredExceptionHandler(
            first: u32,
            handler: unsafe extern "system" fn(*mut ExceptionPointers) -> i32,
        ) -> *mut core::ffi::c_void;
        fn GetModuleHandleW(name: *const u16) -> *mut core::ffi::c_void;
    }

    use std::sync::atomic::{AtomicBool, Ordering};
    // The rich dump itself reads raw stack memory; if that ever faults we
    // must not re-enter and loop. One dump is all a diagnostic needs.
    static DUMPED: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn handler(info: *mut ExceptionPointers) -> i32 {
        let info = &*info;
        let record = &*info.exception_record;
        // 0xC0000005 = ACCESS_VIOLATION
        if record.exception_code == 0xC0000005 {
            let addr = if record.number_parameters >= 2 {
                record.exception_information[1]
            } else {
                0
            };
            let rip = record.exception_address as usize;
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr(),
                "[CRASH] ACCESS_VIOLATION at code=0x{:X} accessing 0x{:X}",
                rip,
                addr
            );
            // Recover the faulting call chain. A RIP of 0 / wild address is
            // a call through a null/garbage function pointer — the pushed
            // return address at [Rsp] points straight at the culprit. We
            // can't symbolize in-process safely from a VEH (no DbgHelp), so
            // emit the module base + raw addresses and their module-relative
            // RVAs; with `--debug-symbols` (#896) producing a PDB these
            // resolve offline via `llvm-symbolizer --obj=<exe> <rva>`.
            if !DUMPED.swap(true, Ordering::SeqCst) && !info.context_record.is_null() {
                let ctx = &*info.context_record;
                let base = GetModuleHandleW(core::ptr::null()) as usize;
                // 256 MiB code window — generous; perry binaries are tens of MiB.
                let win = 0x1000_0000usize;
                let in_mod = |a: usize| base != 0 && a >= base && a < base + win;
                let _ = writeln!(
                    std::io::stderr(),
                    "[CRASH] rip=0x{:X} rsp=0x{:X} module_base=0x{:X}{}",
                    ctx.Rip as usize,
                    ctx.Rsp as usize,
                    base,
                    if in_mod(ctx.Rip as usize) {
                        format!(" rip_rva=+0x{:X}", ctx.Rip as usize - base)
                    } else {
                        String::new()
                    }
                );
                // Scan the top of the faulting stack for return-address-shaped
                // values (anything inside the main module's code window). The
                // first is the immediate caller of the bad call; the rest
                // approximate the chain above it.
                let sp = ctx.Rsp as *const usize;
                let mut printed = 0;
                let mut i = 0usize;
                while i < 512 && printed < 24 {
                    let v = *sp.add(i);
                    if in_mod(v) {
                        let _ = writeln!(
                            std::io::stderr(),
                            "[CRASH] stack[+0x{:X}] = 0x{:X}  rva=+0x{:X}",
                            i * 8,
                            v,
                            v - base
                        );
                        printed += 1;
                    }
                    i += 1;
                }
            }
            let _ = std::io::stderr().flush();
        }
        0 // EXCEPTION_CONTINUE_SEARCH
    }

    #[used]
    #[link_section = ".CRT$XCU"]
    static INSTALL_HANDLER: unsafe extern "C" fn() = {
        unsafe extern "C" fn install() {
            AddVectoredExceptionHandler(1, handler);
        }
        install
    };
}
pub mod clipboard;
pub mod dialog;
pub mod file_dialog;
pub mod folder_dialog;
pub mod keychain;
pub mod layout;
pub mod menu;
pub mod sheet;
pub mod state;
pub mod system;
pub mod toolbar;
pub mod tray;
pub mod widgets;
pub mod window;

pub mod screenshot;

#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;

// FFI exports — split topically into `ffi/*` sub-modules. Each `#[no_mangle]
// pub extern "C" fn perry_ui_<...>` symbol is preserved exactly so codegen-
// generated callsites resolve at link time.
pub mod ffi;
