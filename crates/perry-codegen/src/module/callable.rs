//! A callable worker owns its builders. Only owned fields leave its thread.
use super::*;
use crate::function::OwnedFunction;

#[derive(Clone)]
pub(crate) struct CallableModuleInput {
    target: String,
    prefix: String,
    fp: FpFlags,
    worker: bool,
    local_exec: bool,
    preserve_none: HashSet<String>,
    pub(crate) infos: crate::fn_info::FnInfoState,
    effects: Arc<crate::gc_call_effects::GcEffects>,
}
pub(crate) struct CallableModuleOutput {
    functions: Vec<OwnedFunction>,
    declarations: Vec<(String, String)>,
    globals: Vec<String>,
    constants: Vec<String>,
    string_count: u32,
    ic_count: u32,
    metadata_count: u32,
    alias_scopes: Vec<u32>,
    coalesced: HashMap<String, String>,
    records: Vec<NativeRepRecord>,
    infos: crate::fn_info::FnInfoState,
}
impl CallableModuleInput {
    pub(crate) fn module(&self) -> LlModule {
        let mut module = LlModule::new_with_fp_flags(&self.target, self.fp);
        module.callable_worker = true;
        module.program_has_worker = self.worker;
        module.local_exec_tls = self.local_exec;
        module.set_symbol_prefix(&self.prefix);
        module.set_preserve_none_fns(self.preserve_none.iter().cloned());
        module.set_gc_effects(Arc::clone(&self.effects));
        *module.fn_infos.borrow_mut() = self.infos.clone();
        module
    }
}
impl LlModule {
    pub(crate) fn callable_input(&self) -> CallableModuleInput {
        CallableModuleInput {
            target: self.target_triple.clone(),
            prefix: self.symbol_prefix.clone(),
            fp: self.fp_flags,
            worker: self.program_has_worker,
            local_exec: self.local_exec_tls,
            preserve_none: self.preserve_none_fns.borrow().clone(),
            infos: self.fn_infos.borrow().clone(),
            effects: self.gc_effects(),
        }
    }
    pub(crate) fn into_callable_output(self) -> CallableModuleOutput {
        assert!(
            self.metadata_lines.is_empty(),
            "callable emitted module metadata"
        );
        let functions = self
            .functions
            .into_iter()
            .map(LlFunction::into_owned)
            .collect();
        CallableModuleOutput {
            functions,
            declarations: self.declarations,
            globals: self.globals,
            constants: self.string_constants,
            string_count: self.string_counter,
            ic_count: self.ic_counter,
            metadata_count: self.next_metadata_id - 1,
            alias_scopes: self.buffer_alias_scopes,
            coalesced: self.coalesced_definitions,
            records: self.native_rep_records,
            infos: Rc::try_unwrap(self.fn_infos)
                .unwrap_or_else(|_| panic!("worker info authority escaped"))
                .into_inner(),
        }
    }
    pub(crate) fn merge_callable(
        &mut self,
        output: CallableModuleOutput,
        input: &CallableModuleInput,
        mut symbols: HashMap<String, String>,
    ) {
        use crate::codegen::callable_reloc::Relocations;
        let mut reloc = Relocations::new(
            &self.symbol_prefix,
            self.ic_counter,
            self.next_metadata_id - 1,
            self.string_counter,
        );
        for line in output.globals.iter().chain(&output.constants) {
            reloc.add_definition(line, &mut symbols);
        }
        reloc.symbols = symbols;
        for f in output.functions {
            let mut f = f.into_builder();
            f.relocate_callable(&reloc);
            f.resolve_callable_preinline(|name| self.function_estimated_ir_bytes(name));
            self.add_outlined_helper(f);
        }
        for (name, mut line) in output.declarations {
            if self.declared_names.insert(name.clone()) {
                reloc.text(&mut line);
                self.declarations.push((name, line));
            }
        }
        let mut coalesced = output.coalesced;
        for mut line in output.globals {
            let name =
                global_symbol_name(&line).map(|name| name.trim_start_matches('@').to_string());
            if let Some((name, mut definition)) =
                name.and_then(|name| coalesced.remove(&name).map(|line| (name, line)))
            {
                reloc.text(&mut definition);
                let mut name = name;
                reloc.symbol(&mut name);
                self.add_coalesced_global(&name, definition);
            } else {
                reloc.text(&mut line);
                if is_external_global(&line) {
                    self.add_external_declaration(line);
                } else {
                    self.globals.push(line);
                }
            }
        }
        for mut line in output.constants {
            reloc.text(&mut line);
            self.string_constants.push(line);
        }
        self.ic_counter += output.ic_count;
        let metadata_base = self.next_metadata_id - 1;
        self.buffer_alias_scopes
            .extend(output.alias_scopes.into_iter().map(|id| id + metadata_base));
        self.next_metadata_id += output.metadata_count;
        self.string_counter += output.string_count;
        for mut record in output.records {
            reloc.text(&mut record.llvm_value);
            self.native_rep_records.push(record);
        }
        self.fn_infos
            .borrow_mut()
            .merge_callable(&input.infos, output.infos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callable_merge_preserves_alias_ids_and_coalesced_definitions() {
        for target in ["x86_64-unknown-linux-gnu", "arm64-apple-macosx"] {
            let mut module = LlModule::new(target);
            let mut scopes = module.buffer_alias_scope_allocator();
            assert_eq!(scopes.allocate(), 1);
            module.commit_buffer_alias_scopes(scopes);
            let input = module.callable_input();
            for _ in 0..2 {
                let mut local = input.module();
                let mut scopes = local.buffer_alias_scope_allocator();
                let id = scopes.allocate();
                local.commit_buffer_alias_scopes(scopes);
                let f = local.define_function(
                    format!("f{}", module.function_count()),
                    crate::types::VOID,
                    vec![],
                );
                f.create_block("entry").emit_raw(format!(
                    "; scope !alias.scope !{}, !noalias !{}",
                    id + 1,
                    id + 2
                ));
                local.add_coalesced_global("shared", "@shared = weak_odr global i32 7".into());
                module.merge_callable(local.into_callable_output(), &input, HashMap::new());
            }
            assert_eq!(module.buffer_alias_scopes(), &[1, 4, 7]);
            assert_eq!(module.next_metadata_id, 10);
            let ir = module.to_ir();
            assert!(ir.contains("!alias.scope !5, !noalias !6"));
            assert!(ir.contains("!alias.scope !8, !noalias !9"));
            assert_eq!(ir.matches("@shared = weak_odr").count(), 1);
            assert_eq!(
                ir.matches("$shared = comdat any").count(),
                usize::from(!target.contains("apple"))
            );
        }
    }
}
