//! Compiler-only packing of the realm's GC records into one contiguous run.
//! Aliases preserve each cell's address without emitting an address table.
use super::*;

#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub(crate) name: String,
    pub(crate) ty: String,
    pub(crate) initializer: String,
    pub(crate) size: u64,
    pub(crate) exported: bool,
}

pub(crate) fn begin(module: &mut LlModule) {
    assert!(module.realm_records.is_none(), "nested realm run");
    module.realm_records = Some(Vec::new());
}

pub(crate) fn finish(module: &mut LlModule) -> Result<(), String> {
    let records = module
        .realm_records
        .take()
        .expect("realm run was not begun");
    emit(module, &records)
}

/// All cell composers hand over their typed bytes here. Outside a program
/// run, fixtures and runtime descriptor lists keep their ordinary definition.
pub(crate) fn record(module: &mut LlModule, record: Record, associated: &str) {
    if let Some(records) = module.realm_records.as_mut() {
        assert!(
            associated.is_empty(),
            "descriptor-owned list inside realm run"
        );
        records.push(record);
        return;
    }
    let name = llvm_identifier(&record.name);
    let linkage = if record.exported { "" } else { "internal " };
    module.add_raw_global(format!(
        "@{name} = {linkage}global {} {}, section \"{}\", align 8{associated}",
        record.ty,
        record.initializer,
        section(&module.target_triple)
    ));
}

pub(crate) fn emit(module: &mut LlModule, records: &[Record]) -> Result<(), String> {
    if records.is_empty() {
        return Err("a realm run needs its global cell".into());
    }
    let mut names = std::collections::BTreeSet::new();
    for record in records {
        if !names.insert(&record.name) {
            return Err(format!("duplicate realm cell {}", record.name));
        }
        if record.size < GC_HEADER_SIZE || record.size & 7 != 0 {
            return Err(format!("unaligned realm record {}", record.name));
        }
    }
    let types = records
        .iter()
        .map(|r| r.ty.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let values = records
        .iter()
        .map(|r| format!("{} {}", r.ty, r.initializer))
        .collect::<Vec<_>>()
        .join(", ");
    module.add_raw_global(format!(
        "%PerryRealmRun = type <{{ [0 x i8], {types}, [0 x i8] }}>"
    ));
    let associated = if crate::fn_record::RecordFormat::of(&module.target_triple)
        == crate::fn_record::RecordFormat::Elf
    {
        ", !associated !{ptr @perry_layout_image}"
    } else {
        ""
    };
    module.add_raw_global(format!(
        "@perry_realm_run = global %PerryRealmRun <{{ [0 x i8] zeroinitializer, {values}, [0 x i8] zeroinitializer }}>, section \"{}\", align 8{associated}",
        section(&module.target_triple)
    ));
    for (index, record) in records.iter().enumerate() {
        let name = llvm_identifier(&record.name);
        let linkage = if record.exported { "" } else { "internal " };
        module.add_raw_global(format!(
            "@{name} = {linkage}alias {}, ptr getelementptr (%PerryRealmRun, ptr @perry_realm_run, i32 0, i32 {})",
            record.ty, index + 1
        ));
    }
    for (name, index) in [
        ("perry_realm_run_start", 0),
        ("perry_realm_run_end", records.len() + 1),
    ] {
        module.add_raw_global(format!(
            "@{name} = alias i8, ptr getelementptr (%PerryRealmRun, ptr @perry_realm_run, i32 0, i32 {index})"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn realm_run_keeps_gc_records_contiguous_on_all_targets() {
        use inkwell::{context::Context, targets::TargetData};
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
            "arm64_32-apple-watchos",
        ] {
            let ilp32 = crate::target_layout::target_is_ilp32(triple);
            let (tail_type, tail_value) = if ilp32 {
                ("i32, ", "i32 0, ")
            } else {
                ("", "")
            };
            let object = Record {
                name: "perry_realm_global".into(),
                ty: format!("{{ i8, i8, i16, i32, i32, i32, {tail_type}ptr }}"),
                initializer: format!(
                    "{{ i8 2, i8 5, i16 0, i32 24, i32 0, i32 0, {tail_value}ptr null }}"
                ),
                size: 24,
                exported: true,
            };
            let meta = Record {
                name: "meta".into(),
                ty: "{ i8, i8, i16, i32, [20 x i64] }".into(),
                initializer: "{ i8 19, i8 5, i16 0, i32 168, [20 x i64] zeroinitializer }".into(),
                size: 168,
                exported: false,
            };
            let mut module = LlModule::new(triple);
            module.add_raw_global("@perry_layout_image = constant i64 0".into());
            emit(&mut module, &[object, meta]).unwrap();
            let ir = module.to_ir();
            let context = Context::create();
            let parsed = crate::inprocess::parse_ir_text(&context, &ir, "realm_run").unwrap();
            parsed.verify().unwrap();
            let ty = context.get_struct_type("PerryRealmRun").unwrap();
            let data = TargetData::create(if ilp32 {
                "e-p:32:32-i64:64"
            } else {
                "e-p:64:64-i64:64"
            });
            assert_eq!(data.offset_of_element(&ty, 0), Some(0));
            assert_eq!(data.offset_of_element(&ty, 1), Some(0));
            assert_eq!(data.offset_of_element(&ty, 2), Some(24));
            assert_eq!(data.offset_of_element(&ty, 3), Some(192));
            assert_eq!(data.get_abi_size(&ty), 192);
            assert_eq!(ir.matches("@perry_realm_global = alias").count(), 1);
            assert!(!ir.contains("call "));
        }
    }
}
