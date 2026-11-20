use super::assemble_text;

/// What LLVM 22 prints for Windows x64 and an LLVM 20 assembler rejects: an
/// unwind-v1 epilogue bracketed by `.seh_startepilogue`/`.seh_endepilogue`,
/// and a COMDAT data section with a `unique` id.
const COFF: &str = concat!(
    "\t.def\t@feat.00;\n\t.scl\t3;\n\t.type\t0;\n\t.endef\n",
    "\t.globl\t@feat.00\n@feat.00 = 0\n",
    "\t.text\n",
    "\t.globl\tf\n",
    "\t.p2align\t4\n",
    "f:\n",
    ".seh_proc f\n",
    "\tpushq\t%rsi\n",
    "\t.seh_pushreg %rsi\n",
    "\tsubq\t$64, %rsp\n",
    "\t.seh_stackalloc 64\n",
    "\t.seh_endprologue\n",
    "\tmovl\t%ecx, %eax\n",
    "\t.seh_startepilogue\n",
    "\taddq\t$64, %rsp\n",
    "\tpopq\t%rsi\n",
    "\t.seh_endepilogue\n",
    "\tretq\n",
    "\t.seh_endproc\n",
    "\t.section\t.data,\"dw\",one_only,perry_global_m_ts__0,unique,16\n",
    "\t.globl\tperry_global_m_ts__0\n",
    "\t.p2align\t3, 0x0\n",
    "perry_global_m_ts__0:\n",
    "\t.quad\tf\n",
);

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn coff_text_from_the_bundled_llvm_assembles_in_process() {
    let object = assemble_text(COFF, "x86_64-pc-windows-msvc", &[]).expect("assembles");
    // IMAGE_FILE_MACHINE_AMD64, little-endian, at the start of a COFF object.
    assert_eq!(&object[..2], &[0x64, 0x86]);
    assert!(contains(&object, b".xdata"), "unwind info is emitted");
    assert!(contains(&object, b"perry_global_m_ts__0"));
}

#[test]
fn mach_o_and_elf_text_assemble_in_process() {
    let text = "\t.text\n\t.globl\t_f\n_f:\n\tretq\n";
    let macho = assemble_text(text, "x86_64-apple-macosx13.0.0", &[]).expect("Mach-O");
    assert_eq!(&macho[..4], &[0xcf, 0xfa, 0xed, 0xfe]);
    let text = "\t.text\n\t.globl\tf\nf:\n\tret\n";
    let elf = assemble_text(text, "aarch64-unknown-linux-gnu", &[]).expect("ELF");
    assert_eq!(&elf[..4], b"\x7fELF");
}

/// An error is returned, not printed with the process exiting: LLVM's
/// default handler for an inline-asm error calls `exit(1)`.
#[test]
fn an_unknown_directive_is_an_error_not_an_exit() {
    let error = assemble_text(
        "\t.text\n\t.perry_no_such_directive\n",
        "x86_64-pc-windows-msvc",
        &[],
    )
    .expect_err("unknown directive");
    let message = format!("{error:#}");
    assert!(
        message.contains("perry_no_such_directive") || message.contains("unknown directive"),
        "{message}"
    );
    // The context is still usable afterwards.
    assert!(assemble_text(COFF, "x86_64-pc-windows-msvc", &[]).is_ok());
}
