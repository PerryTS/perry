use super::*;
use std::sync::atomic::Ordering;

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".pushsection .text.perry_eh_site_fixture,\"ax\",@progbits",
    ".p2align 4",
    ".globl perry_eh_site_fixture",
    ".type perry_eh_site_fixture,@function",
    "perry_eh_site_fixture:",
    ".cfi_startproc",
    ".cfi_personality 0x1b, perry_eh_personality",
    ".cfi_lsda 0x1b, .Lsite_lsda",
    "push rbp",
    ".cfi_def_cfa_offset 16",
    ".cfi_offset rbp, -16",
    "mov rbp, rsp",
    ".cfi_def_cfa_register rbp",
    "nop",
    ".Lsite_pad:",
    "nop",
    "leave",
    ".cfi_def_cfa rsp, 8",
    ".cfi_restore rbp",
    "ret",
    ".Lsite_end:",
    ".cfi_endproc",
    ".size perry_eh_site_fixture, .-perry_eh_site_fixture",
    ".popsection",
    ".pushsection .gcc_except_table.site_fixture,\"a\",@progbits",
    ".Lsite_lsda:",
    ".byte 255, 255, 1",
    ".uleb128 .Lsite_calls_end-.Lsite_calls",
    ".Lsite_calls:",
    ".uleb128 0",
    ".uleb128 .Lsite_end-perry_eh_site_fixture",
    ".uleb128 .Lsite_pad-perry_eh_site_fixture",
    ".uleb128 0",
    ".Lsite_calls_end:",
    ".popsection",
);

#[test]
fn one_site_reuses_one_fde_for_the_same_row_and_handler() {
    let _loader = crate::test_support::loaded_objects_test_lock();
    #[cfg(target_arch = "x86_64")]
    let pc = {
        unsafe extern "C" {
            fn perry_eh_site_fixture();
        }
        perry_eh_site_fixture as *const () as u64 + 4
    };
    #[cfg(not(target_arch = "x86_64"))]
    let pc = one_site_reuses_one_fde_for_the_same_row_and_handler as *const () as u64 + 1;
    let walker = Walker::build().expect("walker");
    let image = walker.image_for(pc).expect("test image");
    assert!(image.image.hdr.is_some());
    let before = SITE_FDE_READS.load(Ordering::Relaxed);
    let independent = Site {
        row: image.decode_row(pc),
        handler: image.handler_for(pc),
    };
    assert_eq!(
        SITE_FDE_READS.load(Ordering::Relaxed) - before,
        2,
        "independent control must actually read both FDEs"
    );
    let before = SITE_FDE_READS.load(Ordering::Relaxed);
    let combined = image.decode_site(pc);
    assert_eq!(
        SITE_FDE_READS.load(Ordering::Relaxed) - before,
        1,
        "one Site must decode one FDE"
    );
    let row = |row: Option<StepRow>| {
        row.map(|row| (row.cfa_reg, row.cfa_off, row.reloads, row.args_size))
    };
    assert!(independent.row.is_some(), "a live row must be compared");
    assert_eq!(row(combined.row), row(independent.row));
    assert_eq!(combined.handler, independent.handler);
    #[cfg(target_arch = "x86_64")]
    assert!(
        matches!(combined.handler, Handler::Pad { .. }),
        "a real Perry landing pad must be compared"
    );
    eprintln!("live FDE reads: independent=2 combined=1; register row and handler match");
}
