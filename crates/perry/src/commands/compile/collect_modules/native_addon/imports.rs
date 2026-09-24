//! Read addon import names without requiring a named dylib for every symbol.

use anyhow::{Context, Result};
use object::read::macho::{MachHeader, MachOFile, Nlist};
use object::Object;

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
