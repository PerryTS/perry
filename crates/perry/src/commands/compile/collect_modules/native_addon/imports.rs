//! Read addon import names without requiring a named dylib for every symbol.

use anyhow::{Context, Result};
use object::read::elf::{Dyn, ElfFile, FileHeader};
use object::read::macho::{MachHeader, MachOFile, Nlist};
use object::Object;
use perry_runtime_manifest::{node_api_import_reach, NodeApiReach};
use std::collections::BTreeSet;

/// What an addon binary reaches: a native addon reaches what its Node-API
/// imports reach (the marks of `node_api_host_symbols.txt`). Its import
/// table is its complete reach unless it can find code the table does not
/// name, so it is also open ([`NodeApiReach::World`]) when it
/// - imports a run-time symbol lookup or library load (`dlsym`, `dlopen`,
///   `GetProcAddress`, `LoadLibrary*`): any host export is one call away;
/// - links a library that is neither the host nor the system's: that
///   library's own imports are not read here.
pub fn addon_reach(file: &object::File<'_>) -> Result<BTreeSet<NodeApiReach>> {
    let mut reach = BTreeSet::new();
    for name in plain_import_names(file)? {
        if is_node_api(&name) {
            reach.extend(node_api_import_reach(&name));
        } else if runtime_symbol_lookup(&name) {
            reach.insert(NodeApiReach::World);
        }
    }
    if links_foreign_library(file)? {
        reach.insert(NodeApiReach::World);
    }
    Ok(reach)
}

/// The Node-API symbols the addon imports. For a closed addon they are all
/// the host has to export (an open one can look any export up at run time).
pub fn node_api_imports(file: &object::File<'_>) -> Result<BTreeSet<String>> {
    Ok(plain_import_names(file)?
        .into_iter()
        .filter(|name| is_node_api(name))
        .collect())
}

fn is_node_api(name: &str) -> bool {
    name.starts_with("napi_") || name.starts_with("node_api_")
}

/// Import names as C spells them: Mach-O decorates C names with `_`; ELF
/// may carry `@VERSION`.
fn plain_import_names(file: &object::File<'_>) -> Result<Vec<String>> {
    Ok(imported_symbol_names(file)?
        .into_iter()
        .map(|name| {
            let name = String::from_utf8_lossy(name);
            let name = name.trim_start_matches('_');
            name.split('@').next().unwrap_or_default().to_string()
        })
        .collect())
}

fn runtime_symbol_lookup(name: &str) -> bool {
    matches!(
        name,
        "dlsym"
            | "dlvsym"
            | "dlopen"
            | "dlmopen"
            | "NSLookupSymbolInImage"
            | "NSAddressOfSymbol"
            | "GetProcAddress"
            | "LoadLibraryA"
            | "LoadLibraryW"
            | "LoadLibraryExA"
            | "LoadLibraryExW"
            | "LdrLoadDll"
            | "LdrGetProcedureAddress"
    )
}

/// Does the addon link a library other than the host and the system's?
fn links_foreign_library(file: &object::File<'_>) -> Result<bool> {
    match file {
        object::File::Elf32(file) => elf_links_foreign_library(file),
        object::File::Elf64(file) => elf_links_foreign_library(file),
        object::File::MachO32(file) => macho_links_foreign_library(file),
        object::File::MachO64(file) => macho_links_foreign_library(file),
        _ => {
            // PE names the library of every import. The host is the library
            // the Node-API imports come from.
            let imports = file.imports()?;
            let host = imports
                .iter()
                .filter(|import| {
                    let name = String::from_utf8_lossy(import.name());
                    let name = name.trim_start_matches('_');
                    name.starts_with("napi_") || name.starts_with("node_api_")
                })
                .map(|import| import.library().to_ascii_lowercase())
                .collect::<BTreeSet<_>>();
            Ok(imports.iter().any(|import| {
                let library = import.library().to_ascii_lowercase();
                !host.contains(&library) && !windows_system_library(&library)
            }))
        }
    }
}

fn elf_links_foreign_library<Elf: FileHeader<Endian = object::Endianness>>(
    file: &ElfFile<'_, Elf>,
) -> Result<bool> {
    let endian = file.endian();
    let sections = file.elf_section_table();
    // A shared object without a dynamic section is not one the loader reads
    // the way it is read here.
    let Some((dynamic, strings_index)) = sections.dynamic(endian, file.data())? else {
        return Ok(true);
    };
    let strings = sections.strings(endian, file.data(), strings_index)?;
    for entry in dynamic {
        if entry.tag32(endian) == Some(object::elf::DT_NEEDED) {
            let library = entry.string(endian, strings)?;
            if !elf_system_library(&String::from_utf8_lossy(library)) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn macho_links_foreign_library<M: MachHeader>(file: &MachOFile<'_, M>) -> Result<bool> {
    let endian = file.endian();
    let mut commands = file.macho_load_commands()?;
    while let Some(command) = commands.next()? {
        if let Some(dylib) = command.dylib()? {
            let path = command.string(endian, dylib.dylib.name)?;
            let path = String::from_utf8_lossy(path);
            if !path.starts_with("/usr/lib/") && !path.starts_with("/System/Library/") {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn elf_system_library(soname: &str) -> bool {
    [
        "libc.so",
        "libm.so",
        "libdl.so",
        "libpthread.so",
        "librt.so",
        "libutil.so",
        "libresolv.so",
        "libgcc_s.so",
        "libatomic.so",
        "libstdc++.so",
        "libc++.so",
        "libc++abi.so",
        "ld-linux",
        "ld-musl",
        "libc.musl",
    ]
    .iter()
    .any(|prefix| soname.starts_with(prefix))
}

fn windows_system_library(dll: &[u8]) -> bool {
    let dll = String::from_utf8_lossy(dll);
    ["api-ms-win-", "ext-ms-", "vcruntime", "msvcp", "ucrtbase"]
        .iter()
        .any(|prefix| dll.starts_with(prefix))
        || matches!(
            dll.as_ref(),
            "kernel32.dll"
                | "kernelbase.dll"
                | "ntdll.dll"
                | "msvcrt.dll"
                | "advapi32.dll"
                | "user32.dll"
                | "shell32.dll"
                | "ole32.dll"
                | "oleaut32.dll"
                | "userenv.dll"
                | "ws2_32.dll"
                | "bcrypt.dll"
                | "bcryptprimitives.dll"
                | "crypt32.dll"
                | "secur32.dll"
                | "ncrypt.dll"
                | "psapi.dll"
                | "iphlpapi.dll"
                | "dbghelp.dll"
                | "shlwapi.dll"
                | "synchronization.dll"
        )
}

/// Mach-O dynamic-lookup and executable ordinals refer to the loading host,
/// not entries in the dylib table. `object::File::imports` in object 0.37
/// rejects those ordinals. Read the same declared import-symbol range without
/// resolving libraries; PE/ELF retain their normal import-table handling.
pub fn imported_symbol_names<'data>(file: &object::File<'data>) -> Result<Vec<&'data [u8]>> {
    match file {
        object::File::MachO32(file) => macho_import_names(file),
        object::File::MachO64(file) => macho_import_names(file),
        _ => Ok(file
            .imports()?
            .into_iter()
            .map(|import| import.name())
            .collect()),
    }
}

fn macho_import_names<'data, M: MachHeader>(
    file: &MachOFile<'data, M>,
) -> Result<Vec<&'data [u8]>> {
    let endian = file.endian();
    let symbols = file.macho_symbol_table();
    let mut commands = file.macho_load_commands()?;
    let mut names = Vec::new();
    while let Some(command) = commands.next()? {
        if let Some(table) = command.dysymtab()? {
            let start = table.iundefsym.get(endian) as usize;
            let count = table.nundefsym.get(endian) as usize;
            let end = start
                .checked_add(count)
                .context("Mach-O import symbol range overflow")?;
            // Use LC_DYSYMTAB rather than ObjectSymbol::is_undefined: the
            // declared range also includes prebound undefined symbols.
            for index in start..end {
                names.push(
                    symbols
                        .symbol(object::SymbolIndex(index))?
                        .name(endian, symbols.strings())?,
                );
            }
        }
    }
    Ok(names)
}
