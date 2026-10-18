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
    alias_count: u32,
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
            alias_count: self.buffer_alias_counter,
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
            self.buffer_alias_counter,
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
        for mut line in output.globals {
            reloc.text(&mut line);
            self.globals.push(line);
        }
        for mut line in output.constants {
            reloc.text(&mut line);
            self.string_constants.push(line);
        }
        self.ic_counter += output.ic_count;
        self.buffer_alias_counter += output.alias_count;
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
