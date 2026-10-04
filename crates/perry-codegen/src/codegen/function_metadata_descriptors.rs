//! Compact, relocation-free tables for generated function metadata.
//!
//! Absolute pointers in a read-only table need loader relocations.  These
//! descriptors instead store signed offsets from the table itself, matching
//! the representation used by Perry's compact GC map.  Generated code makes
//! one runtime call per non-empty table; the runtime reconstructs the pointers
//! and inserts the entries while holding the registry lock once.

use crate::module::LlModule;

pub(super) struct DescriptorTable {
    pub(super) global: String,
    pub(super) len: usize,
}

fn table_section(target_triple: &str) -> &'static str {
    if target_triple.contains("apple") {
        "__TEXT,__const"
    } else if target_triple.contains("windows") {
        ".rdata$perry_fn_metadata"
    } else {
        // LLVM conservatively marks relative expressions that mention an
        // externally visible function as requiring a relocation.  Keep the
        // table in RELRO: the static linker resolves Perry executable symbols,
        // and the loader maps the section read-only after any relocations.
        ".data.rel.ro.perry_fn_metadata"
    }
}

fn relative_i32(pointer: &str, table: &str) -> String {
    let pointer = if pointer.starts_with('@') || pointer.starts_with("getelementptr ") {
        pointer.to_owned()
    } else {
        format!("@{pointer}")
    };
    format!(
        "i32 trunc (i64 sub (i64 ptrtoint (ptr {pointer} to i64), \
         i64 ptrtoint (ptr @{table} to i64)) to i32)"
    )
}

/// Emit `{ function offset, name-bytes offset, byte length }` records.
pub(super) fn emit_name_table<'a>(
    module: &mut LlModule,
    module_prefix: &str,
    entries: impl ExactSizeIterator<Item = (&'a str, &'a str, usize)>,
) -> Option<DescriptorTable> {
    let len = entries.len();
    if len == 0 {
        return None;
    }
    let global = format!("__perry_function_name_descriptors_{module_prefix}");
    let ty = "{ i32, i32, i32 }";
    let records = entries
        .map(|(function, bytes, byte_len)| {
            format!(
                "{ty} {{ {}, {}, i32 {byte_len} }}",
                relative_i32(function, &global),
                relative_i32(bytes, &global),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let section = table_section(&module.target_triple);
    module.add_raw_global(format!(
        "@{global} = private constant [{len} x {ty}] [{records}], section \"{section}\", align 4"
    ));
    Some(DescriptorTable { global, len })
}

/// Emit `{ function offset, source-bytes offset, byte length, kind flag }`
/// records.
pub(super) fn emit_source_table<'a>(
    module: &mut LlModule,
    module_prefix: &str,
    entries: impl ExactSizeIterator<Item = (&'a str, String, usize, bool)>,
) -> Option<DescriptorTable> {
    let len = entries.len();
    if len == 0 {
        return None;
    }
    let global = format!("__perry_function_source_descriptors_{module_prefix}");
    let ty = "{ i32, i32, i32, i32 }";
    let records = entries
        .map(|(function, bytes, byte_len, is_non_strict_ordinary)| {
            format!(
                "{ty} {{ {}, {}, i32 {byte_len}, i32 {} }}",
                relative_i32(function, &global),
                relative_i32(&bytes, &global),
                i32::from(is_non_strict_ordinary),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let section = table_section(&module.target_triple);
    module.add_raw_global(format!(
        "@{global} = private constant [{len} x {ty}] [{records}], section \"{section}\", align 4"
    ));
    Some(DescriptorTable { global, len })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_use_relative_i32_fields_without_absolute_pointer_elements() {
        let mut module = LlModule::new("x86_64-unknown-linux-gnu");
        emit_name_table(
            &mut module,
            "fixture_ts",
            [("function_a", "name_a", 6), ("function_b", "name_b", 4)].into_iter(),
        );
        emit_source_table(
            &mut module,
            "fixture_ts",
            [(
                "function_a",
                "getelementptr (i8, ptr @source_a, i64 3)".to_string(),
                20,
                true,
            )]
            .into_iter(),
        );
        let ir = module.to_ir();
        assert!(
            ir.contains("@__perry_function_name_descriptors_fixture_ts = private constant [2 x")
        );
        assert!(
            ir.contains("@__perry_function_source_descriptors_fixture_ts = private constant [1 x")
        );
        assert_eq!(
            ir.matches("section \".data.rel.ro.perry_fn_metadata\"")
                .count(),
            2
        );
        assert_eq!(ir.matches("i32 trunc (i64 sub").count(), 6);
        assert_eq!(ir.matches("ptrtoint (ptr @function_a").count(), 2);
        assert!(!ir.contains("{ ptr @function_a"));
        assert!(!ir.contains("{ ptr @name_a"));
    }
}
