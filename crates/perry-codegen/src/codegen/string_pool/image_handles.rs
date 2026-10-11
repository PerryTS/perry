use crate::block::LlBlock;
use crate::module::LlModule;
use crate::strings::StringEntry;
use crate::types::{DOUBLE, I8, I64, VOID};

pub(super) fn target_uses_runtime_boxed_images(triple: &str) -> bool {
    matches!(
        crate::codegen::target_os_of(triple),
        Some("macos" | "ios" | "tvos" | "watchos" | "visionos")
    )
}

pub(super) fn add_mutable_string_handle(
    llmod: &mut LlModule,
    entry: &StringEntry,
    agent_strings_tls: bool,
) {
    if agent_strings_tls {
        llmod.add_internal_thread_local_global(&entry.handle_global, DOUBLE, "0.0");
    } else {
        llmod.add_internal_global(&entry.handle_global, DOUBLE, "0.0");
    }
}

pub(super) fn emit_runtime_boxed_image_handle(blk: &mut LlBlock, entry: &StringEntry) -> String {
    let literal = format!("@{}", super::image::name(entry));
    let header_offset = crate::target_layout::GC_HEADER_SIZE_BYTES.to_string();
    let pointer = blk.gep(crate::types::I8, &literal, &[(I64, &header_offset)]);
    let handle = blk.ptrtoint(&pointer, I64);
    blk.call(DOUBLE, "js_nanbox_string", &[(I64, &handle)])
}

pub(super) fn emit_prepare_agent_strings(
    llmod: &mut LlModule,
    agent_strings_name: &str,
    module_prefix: &str,
    needs_preparation: bool,
    agent_strings_tls: bool,
    string_chunks: &[String],
) {
    if !needs_preparation {
        llmod
            .define_function(agent_strings_name, VOID, vec![])
            .create_block("entry")
            .ret_void();
        return;
    }

    let ready = format!("__perry_agent_strings_ready_{}", module_prefix);
    if agent_strings_tls {
        llmod.add_internal_thread_local_global(&ready, I8, "0");
    } else {
        llmod.add_internal_global(&ready, I8, "0");
    }
    let prepare_strings = llmod.define_function(agent_strings_name, VOID, vec![]);
    prepare_strings.create_block("entry");
    prepare_strings.create_block("prepare");
    prepare_strings.create_block("done");
    let prepare_label = prepare_strings.block_mut(1).unwrap().label.clone();
    let done_label = prepare_strings.block_mut(2).unwrap().label.clone();
    let blk = prepare_strings.block_mut(0).unwrap();
    let prepared = blk.load(I8, &format!("@{}", ready));
    let prepared = blk.icmp_ne(I8, &prepared, "0");
    blk.cond_br(&prepared, &done_label, &prepare_label);
    let blk = prepare_strings.block_mut(1).unwrap();
    for name in string_chunks {
        blk.call_void(name, &[]);
    }
    blk.store(I8, "1", &format!("@{}", ready));
    blk.br(&done_label);
    prepare_strings.block_mut(2).unwrap().ret_void();
}
