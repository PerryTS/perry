//! Phase 0 feasibility spike for the in-process LLVM backend (exp/llvm-inprocess).
//!
//! Three modes:
//!
//! * `--version`            — prints a clang-compatible banner, so Perry's
//!                            `find_clang`/`ensure_supported_clang` accept this
//!                            binary via `PERRY_LLVM_CLANG`.
//! * `demo <outdir>`        — builds a module through the inkwell builder API
//!                            exercising every construct the experiment brief
//!                            flags as a potential blocker (NaN-box constants,
//!                            f64<->i64 bitcasts, inline asm with exact
//!                            constraint strings, module-level asm, appending
//!                            `@llvm.used`, `gc "statepoint-example"`), then
//!                            verifies, emits a .o, links it with `cc`, runs
//!                            it, and checks the output.
//! * `<clang-style argv>`   — the shim: accepts exactly the argv Perry's
//!                            `build_clang_compile_plan` produces
//!                            (`-c -O3 -fno-math-errno [...] x.ll -o x.o
//!                            -target <triple>`), but compiles **in-process**:
//!                            parse IR -> verify -> `default<O3>` pass
//!                            pipeline -> TargetMachine object emission.
//!                            Setting `PERRY_LLVM_CLANG` to this binary swaps
//!                            the whole Perry compile onto the in-process
//!                            backend with zero Perry changes — the Phase 0
//!                            A/B harness.
//!
//! Liveness proof (the "gate must assert its subject was live" rule): every
//! shim compile appends a line to `$PERRY_LLVMC_SPIKE_LOG` when set. An A/B
//! arm that claims to be in-process must show a non-empty log.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use inkwell::context::Context;
use inkwell::memory_buffer::MemoryBuffer;
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::values::AsValueRef;
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};

// NaN-box tags, mirrored from perry-runtime/src/value.rs.
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const INT32_TAG_SHIFTED: u64 = 0x7FFE_0000_0000_002A; // INT32_TAG with payload 42

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-v") {
        print_version_banner();
        return ExitCode::SUCCESS;
    }
    match args.first().map(String::as_str) {
        Some("demo") => {
            let outdir = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            match run_demo(&outdir) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("demo failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(_) => match run_shim(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("perry-llvmc-spike: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            eprintln!("usage: perry-llvmc-spike [--version | demo <outdir> | <clang-style args>]");
            ExitCode::FAILURE
        }
    }
}

fn print_version_banner() {
    // `parse_clang_major_version` looks for "clang version <digits>"; report
    // the LLVM major we actually embed so `ensure_supported_clang` sees a
    // supported opaque-pointer toolchain.
    let triple = TargetMachine::get_default_triple();
    println!(
        "clang version 22.1.4 (perry-llvmc-spike in-process LLVM backend)\n\
         Target: {}\n\
         Thread model: posix\n\
         InstalledDir: (in-process)",
        triple.as_str().to_string_lossy()
    );
}

// ---------------------------------------------------------------------------
// Shim mode: clang-compatible argv, in-process compile
// ---------------------------------------------------------------------------

struct ShimArgs {
    input: PathBuf,
    output: PathBuf,
    opt: char, // '0', 's', '3', ...
    target: Option<String>,
    mcpu_native: bool,
    explicit_cpu: Option<String>,
    mllvm: Vec<String>,
}

fn parse_shim_args(args: &[String]) -> Result<ShimArgs, String> {
    let mut input = None;
    let mut output = None;
    let mut opt = '0';
    let mut target = None;
    let mut mcpu_native = false;
    let mut explicit_cpu = None;
    let mut mllvm = Vec::new();
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-c" | "-fno-math-errno" | "-g" => {} // -g: parity with clang — no DI metadata in Perry IR, so it adds nothing (#7144)
            "-o" => output = it.next().cloned().map(PathBuf::from),
            "-target" => target = it.next().cloned(),
            "-mllvm" => {
                if let Some(f) = it.next() {
                    mllvm.push(f.clone());
                }
            }
            s if s.starts_with("-O") => opt = s.chars().nth(2).unwrap_or('0'),
            "-mcpu=native" | "-march=native" => mcpu_native = true,
            s if s.starts_with("-mcpu=") => explicit_cpu = Some(s["-mcpu=".len()..].to_string()),
            s if s.starts_with("-march=") => explicit_cpu = Some(s["-march=".len()..].to_string()),
            s if !s.starts_with('-') => input = Some(PathBuf::from(s)),
            other => return Err(format!("unrecognized clang arg: {other}")),
        }
    }
    Ok(ShimArgs {
        input: input.ok_or("missing input .ll")?,
        output: output.ok_or("missing -o output")?,
        opt,
        target,
        mcpu_native,
        explicit_cpu,
        mllvm,
    })
}

fn run_shim(args: &[String]) -> Result<(), String> {
    let t0 = Instant::now();
    let sa = parse_shim_args(args)?;

    // -mllvm flags (e.g. -inlinehint-threshold=N) go through the cl::opt
    // parser, same effect as clang's -mllvm passthrough. Process-global, which
    // is fine here (one compile per process) but is a real constraint for the
    // integrated backend — noted in the write-up.
    if !sa.mllvm.is_empty() {
        let mut argv: Vec<CString> = vec![CString::new("perry-llvmc-spike").unwrap()];
        for f in &sa.mllvm {
            argv.push(CString::new(f.as_str()).map_err(|e| e.to_string())?);
        }
        let ptrs: Vec<*const std::os::raw::c_char> = argv.iter().map(|c| c.as_ptr()).collect();
        unsafe {
            llvm_sys::support::LLVMParseCommandLineOptions(
                ptrs.len() as i32,
                ptrs.as_ptr(),
                std::ptr::null(),
            );
        }
    }

    Target::initialize_all(&InitializationConfig::default());
    let context = Context::create();
    let buf = MemoryBuffer::create_from_file(&sa.input)
        .map_err(|e| format!("read {}: {}", sa.input.display(), e))?;
    let ir_bytes = buf.get_size();
    let module = context
        .create_module_from_ir(buf)
        .map_err(|e| format!("LLVM IR parse error in {}:\n{}", sa.input.display(), e))?;

    module
        .verify()
        .map_err(|e| format!("verifier rejected {}:\n{}", sa.input.display(), e))?;

    let triple_str = sa
        .target
        .clone()
        .unwrap_or_else(|| TargetMachine::get_default_triple().as_str().to_string_lossy().into_owned());
    let triple = TargetTriple::create(&triple_str);
    let target =
        Target::from_triple(&triple).map_err(|e| format!("no target for {triple_str}: {e}"))?;

    let (cpu, features) = if sa.mcpu_native {
        (
            TargetMachine::get_host_cpu_name().to_string_lossy().into_owned(),
            TargetMachine::get_host_cpu_features().to_string_lossy().into_owned(),
        )
    } else if let Some(cpu) = &sa.explicit_cpu {
        (cpu.clone(), String::new())
    } else {
        (String::new(), String::new())
    };

    let opt_level = match sa.opt {
        '0' => OptimizationLevel::None,
        '1' => OptimizationLevel::Less,
        '2' | 's' | 'z' => OptimizationLevel::Default,
        _ => OptimizationLevel::Aggressive,
    };
    let tm = target
        .create_target_machine(&triple, &cpu, &features, opt_level, RelocMode::PIC, CodeModel::Default)
        .ok_or("failed to create TargetMachine")?;

    // Match clang's behavior of trusting -target over the module's own triple,
    // and give the module the machine's real datalayout before optimizing.
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());

    let pipeline = match sa.opt {
        '0' => "default<O0>",
        '1' => "default<O1>",
        '2' => "default<O2>",
        's' => "default<Os>",
        'z' => "default<Oz>",
        _ => "default<O3>",
    };
    module
        .run_passes(pipeline, &tm, PassBuilderOptions::create())
        .map_err(|e| format!("pass pipeline {pipeline} failed: {e}"))?;

    tm.write_to_file(&module, FileType::Object, &sa.output)
        .map_err(|e| format!("object emission to {} failed: {}", sa.output.display(), e))?;

    if let Ok(log) = std::env::var("PERRY_LLVMC_SPIKE_LOG") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
            let _ = writeln!(
                f,
                "inprocess-compile input={} ir_bytes={} opt=-O{} cpu={} target={} ms={}",
                sa.input.display(),
                ir_bytes,
                sa.opt,
                if cpu.is_empty() { "(default)" } else { &cpu },
                triple_str,
                t0.elapsed().as_millis()
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Demo mode: builder-API construction of the brief's "can it express X" list
// ---------------------------------------------------------------------------

fn run_demo(outdir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(outdir).map_err(|e| e.to_string())?;
    Target::initialize_all(&InitializationConfig::default());

    let context = Context::create();
    let module = context.create_module("spike_demo");
    let builder = context.create_builder();
    let i64t = context.i64_type();
    let f64t = context.f64_type();
    let i32t = context.i32_type();
    let ptrt = context.ptr_type(AddressSpace::default());

    // (1) NaN-box constant fidelity: TAG_UNDEFINED as a double constant built
    // from raw bits. If f64::from_bits -> const_float loses NaN payload bits,
    // the whole constant-emission strategy must change; the compiled program
    // prints the bits so we can check end to end.
    let undef_const = f64t.const_float(f64::from_bits(TAG_UNDEFINED));
    let int42_const = f64t.const_float(f64::from_bits(INT32_TAG_SHIFTED));

    // (2) f64 -> i64 bitcast in a function: `nanbox_bits(double) -> i64`.
    let bits_fn_ty = i64t.fn_type(&[f64t.into()], false);
    let bits_fn = module.add_function("nanbox_bits", bits_fn_ty, None);
    {
        let entry = context.append_basic_block(bits_fn, "entry");
        builder.position_at_end(entry);
        let v = bits_fn.get_nth_param(0).unwrap().into_float_value();
        let as_bits = builder
            .build_bit_cast(v, i64t, "bits")
            .map_err(|e| e.to_string())?
            .into_int_value();
        builder.build_return(Some(&as_bits)).map_err(|e| e.to_string())?;
    }

    // (6) gc attribute — inkwell has no wrapper; llvm-sys expresses it.
    let gc_fn = module.add_function("gc_marked", i64t.fn_type(&[i64t.into()], false), None);
    unsafe {
        let name = CString::new("statepoint-example").unwrap();
        llvm_sys::core::LLVMSetGC(gc_fn.as_value_ref(), name.as_ptr());
    }
    {
        let entry = context.append_basic_block(gc_fn, "entry");
        builder.position_at_end(entry);
        let p = gc_fn.get_nth_param(0).unwrap().into_int_value();
        let one = i64t.const_int(1, false);
        let r = builder.build_int_add(p, one, "r").map_err(|e| e.to_string())?;
        builder.build_return(Some(&r)).map_err(|e| e.to_string())?;
    }

    // (7) extern varargs declaration, same shape as Perry's runtime calls.
    let printf_ty = i32t.fn_type(&[ptrt.into()], true);
    let printf = module.add_function("printf", printf_ty, Some(Linkage::External));

    // main
    let main_fn = module.add_function("main", i32t.fn_type(&[], false), None);
    let entry = context.append_basic_block(main_fn, "entry");
    builder.position_at_end(entry);

    // (3) the exact inline-asm barrier Perry emits: `call void asm sideeffect "", ""()`.
    let void_fn_ty = context.void_type().fn_type(&[], false);
    let asm = context.create_inline_asm(
        void_fn_ty,
        String::new(),
        String::new(),
        true,  // sideeffect
        false, // alignstack
        None,  // dialect
        false, // can_throw
    );
    builder
        .build_indirect_call(void_fn_ty, asm, &[], "barrier")
        .map_err(|e| e.to_string())?;

    // Format string global.
    let fmt = builder
        .build_global_string_ptr("tag_undefined=%llx int42=%llx gc=%lld cmp=%d\n", "fmt")
        .map_err(|e| e.to_string())?;

    // Push the NaN-box constants through the bitcast fn and print their bits.
    let undef_bits = builder
        .build_call(bits_fn, &[undef_const.into()], "u")
        .map_err(|e| e.to_string())?
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();
    let int42_bits = builder
        .build_call(bits_fn, &[int42_const.into()], "i")
        .map_err(|e| e.to_string())?
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();
    let gc_out = builder
        .build_call(gc_fn, &[i64t.const_int(41, false).into()], "g")
        .map_err(|e| e.to_string())?
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();
    // Tag-check pattern from generated code: (bits >> 32) == 0x7FFE ?
    let hi = builder
        .build_right_shift(int42_bits, i64t.const_int(32, false), false, "hi")
        .map_err(|e| e.to_string())?;
    let is_i32 = builder
        .build_int_compare(IntPredicate::EQ, hi, i64t.const_int(0x7FFE_0000, false), "isi32")
        .map_err(|e| e.to_string())?;
    let is_i32_ext = builder
        .build_int_z_extend(is_i32, i32t, "isi32e")
        .map_err(|e| e.to_string())?;

    builder
        .build_call(
            printf,
            &[
                fmt.as_pointer_value().into(),
                undef_bits.into(),
                int42_bits.into(),
                gc_out.into(),
                is_i32_ext.into(),
            ],
            "p",
        )
        .map_err(|e| e.to_string())?;
    builder
        .build_return(Some(&i32t.const_zero()))
        .map_err(|e| e.to_string())?;

    // (5) appending @llvm.used keeping gc_marked alive (the Mach-O
    // no-dead-strip mechanism Perry's runtime relies on for anchors).
    let used_ty = ptrt.array_type(1);
    let used = module.add_global(used_ty, None, "llvm.used");
    used.set_linkage(Linkage::Appending);
    used.set_section(Some("llvm.metadata"));
    used.set_initializer(&ptrt.const_array(&[gc_fn.as_global_value().as_pointer_value()]));

    // (4) module-level asm defining a real symbol.
    module.set_inline_assembly(
        ".globl _perry_spike_asm_marker\n_perry_spike_asm_marker:\n.quad 0xC0FFEE\n",
    );

    module.verify().map_err(|e| format!("verifier: {e}"))?;

    // Debug view — printed from the real module, exactly the thesis.
    let ll_path = outdir.join("spike_demo.ll");
    std::fs::write(&ll_path, module.print_to_string().to_string()).map_err(|e| e.to_string())?;

    // Optimize + emit through the same path the shim uses.
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
    let tm = target
        .create_target_machine(
            &triple,
            &TargetMachine::get_host_cpu_name().to_string_lossy(),
            &TargetMachine::get_host_cpu_features().to_string_lossy(),
            OptimizationLevel::Aggressive,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or("failed to create TargetMachine")?;
    module.set_data_layout(&tm.get_target_data().get_data_layout());
    module
        .run_passes("default<O3>", &tm, PassBuilderOptions::create())
        .map_err(|e| format!("passes: {e}"))?;

    let obj_path = outdir.join("spike_demo.o");
    tm.write_to_file(&module, FileType::Object, &obj_path)
        .map_err(|e| e.to_string())?;

    // Link + run + check.
    let bin_path = outdir.join("spike_demo");
    let link = Command::new("cc")
        .arg(&obj_path)
        .arg("-o")
        .arg(&bin_path)
        .output()
        .map_err(|e| e.to_string())?;
    if !link.status.success() {
        return Err(format!(
            "link failed:\n{}",
            String::from_utf8_lossy(&link.stderr)
        ));
    }
    let run = Command::new(&bin_path).output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&run.stdout);
    println!("--- compiled program output ---\n{stdout}-------------------------------");

    let expect_undef = format!("{:x}", TAG_UNDEFINED);
    let expect_i42 = format!("{:x}", INT32_TAG_SHIFTED);
    if !stdout.contains(&expect_undef) || !stdout.contains(&expect_i42) {
        return Err(format!(
            "NaN-box constant bits were NOT preserved: expected {expect_undef} and {expect_i42} in output"
        ));
    }
    if !stdout.contains("gc=42") || !stdout.contains("cmp=1") {
        return Err("gc-marked function or tag-check produced wrong values".into());
    }

    // The module-asm symbol must exist in the object.
    let nm = Command::new("nm").arg(&obj_path).output().map_err(|e| e.to_string())?;
    let nm_out = String::from_utf8_lossy(&nm.stdout);
    if !nm_out.contains("_perry_spike_asm_marker") {
        return Err("module-level asm symbol missing from object".into());
    }

    println!(
        "demo OK: NaN-box constants preserved, inline asm + module asm + llvm.used + gc attr \
         all expressed; artifacts in {}",
        outdir.display()
    );
    Ok(())
}
