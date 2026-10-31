//! Link every listed function section, including Rust archive members, into
//! one hot prefix. Without an order this adds no arguments or input files.
use anyhow::Result;
use std::{fs, path::Path, process::Command};

pub(super) fn add_link_args(
    cmd: &mut Command,
    cache: &Path,
    linux: bool,
    android: bool,
    harmony: bool,
    direct_apple: bool,
    xlinker_apple: bool,
) -> Result<()> {
    let dir = cache.join("function-order");
    let elf = linux && !android && !harmony;
    let order = match perry_codegen::program_function_layout() {
        perry_codegen::FunctionLayout::Order(order) if !order.is_empty() => order,
        _ => {
            // Every JS body has its own input section (so the link can drop
            // each one nothing references); gather them into the one JS text
            // output section whose bounds the stack capture reads.
            if elf {
                fs::create_dir_all(&dir)?;
                let rule = perry_codegen::function_order::JS_TEXT_LINK_RULE;
                let script = dir.join("js-text.v1.ld");
                if fs::read_to_string(&script).ok().as_deref() != Some(rule) {
                    write_atomically(&script, rule)?;
                }
                cmd.arg(format!("-Wl,-T,{}", script.display()));
            }
            return Ok(());
        }
    };
    fs::create_dir_all(&dir)?;
    if elf {
        let script = dir.join(format!("{:016x}.v5.ld", order.digest()));
        fs::write(
            &script,
            order.elf_linker_script().map_err(anyhow::Error::msg)?,
        )?;
        cmd.arg(format!("-Wl,-T,{}", script.display()));
        // Preserve ordinary ELF geometry. The native mapper selects complete
        // aligned pages inside the actual selected span after PIE relocation.
        cmd.arg("-Wl,-z,common-page-size=0x1000");
        cmd.arg("-Wl,-z,separate-code");
    } else {
        let lld = android || harmony;
        let file = dir.join(format!(
            "{:016x}.{}.txt",
            order.digest(),
            if lld { "lld" } else { "ld64" }
        ));
        fs::write(&file, order.linker_ordering_file(!lld))?;
        if lld {
            cmd.arg(format!("-Wl,--symbol-ordering-file={}", file.display()));
            cmd.arg("-Wl,--no-warn-symbol-ordering");
        } else if direct_apple {
            cmd.arg("-order_file").arg(&file);
        } else if xlinker_apple {
            cmd.arg("-Xlinker")
                .arg("-order_file")
                .arg("-Xlinker")
                .arg(&file);
        } else {
            cmd.arg(format!("-Wl,-order_file,{}", file.display()));
        }
    }
    Ok(())
}

/// Concurrent links share the cache: a reader never sees a half-written
/// script (write a sibling, then rename over the final name).
fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
