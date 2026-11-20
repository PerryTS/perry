//! `parse_ir_text` must not keep the IR text it parsed.
//!
//! The parse goes through a memory buffer holding a copy of the text. The LLVM
//! entry point used does not consume that buffer, so the caller owns it; a
//! parse that forgets it leaks one full copy of the text per module, which
//! adds up to gigabytes over the hundreds of codegen units of a large program.
//!
//! The probe reads the resident set of the whole process, so it runs in a
//! process of its own: inside the shared test binary every other test thread
//! allocates and frees concurrently, and their growth lands in the same
//! number (CI saw 317 MB of growth over a run that leaks nothing).
use inkwell::context::Context;

/// Set in the child process that runs the measurement.
const PROBE_ENV: &str = "PERRY_PARSE_IR_TEXT_LEAK_PROBE";

/// Resident set size of this process in bytes (Linux only).
#[cfg(target_os = "linux")]
fn resident_bytes() -> usize {
    let statm = std::fs::read_to_string("/proc/self/statm").expect("procfs");
    let pages: usize = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
    pages * 4096
}

fn big_ir(functions: usize) -> String {
    let mut ir = String::with_capacity(functions * 160);
    for i in 0..functions {
        ir.push_str(&format!(
            "define i64 @f{i}(i64 %a, i64 %b) {{\nentry:\n  %x = add i64 %a, %b\n  %y = mul i64 %x, {i}\n  ret i64 %y\n}}\n"
        ));
    }
    ir
}

/// The measurement itself; runs only in the child process.
fn measure_parse_growth() {
    let ir = big_ir(60_000);
    assert!(ir.len() > 4_000_000);
    let parse = || {
        let context = Context::create();
        let module = super::parse_ir_text(&context, &ir, "leak_probe").expect("parses");
        assert!(module.get_function("f59999").is_some());
    };
    // Warm up allocator arenas, then measure growth over repeated parses.
    parse();
    #[cfg(target_os = "linux")]
    let before = resident_bytes();
    for _ in 0..30 {
        parse();
    }
    #[cfg(target_os = "linux")]
    {
        let grown = resident_bytes().saturating_sub(before);
        // A leaked copy of the text is >4 MB per parse (>120 MB here).
        assert!(
            grown < 48 * 1024 * 1024,
            "30 parses of a {} byte module grew the process by {} bytes",
            ir.len(),
            grown
        );
    }
}

#[test]
fn parsing_ir_text_does_not_leak_the_text_buffer() {
    if std::env::var_os(PROBE_ENV).is_some() {
        measure_parse_growth();
        return;
    }
    let module = module_path!();
    let test = format!(
        "{}::parsing_ir_text_does_not_leak_the_text_buffer",
        module.split_once("::").map_or(module, |(_, rest)| rest)
    );
    let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([test.as_str(), "--exact", "--test-threads=1"])
        .env(PROBE_ENV, "1")
        .output()
        .expect("the probe process runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "the leak probe failed:\n{stdout}\n{stderr}"
    );
    // The child ran exactly this test (a filter that matched nothing passes
    // too, and would prove nothing).
    assert!(
        stdout.contains("test result: ok. 1 passed"),
        "the leak probe did not run `{test}`:\n{stdout}\n{stderr}"
    );
}
