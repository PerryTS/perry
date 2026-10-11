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
    let perry_codegen::FunctionLayout::Order(order) = perry_codegen::program_function_layout()
    else {
        return Ok(());
    };
    if order.is_empty() {
        return Ok(());
    }
    let dir = cache.join("function-order");
    fs::create_dir_all(&dir)?;
    if linux && !android && !harmony {
        let script = dir.join(format!("{:016x}.v4.ld", order.digest()));
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
