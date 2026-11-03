//! Ordered literal/class initialization chunks.

use crate::block::LlBlock;
use crate::module::LlModule;
use crate::types::VOID;

/// Emits a long sequence of INDEPENDENT init operations (string allocation,
/// closure/class/function registration) into a series of small chunk functions
/// instead of one giant function, then exposes the chunk names so the entry
/// `__perry_init_strings_*` can call them in order (#5391 function splitting).
///
/// A large bundle interns ~190K strings and registers tens of thousands of
/// closures/classes; emitting all of it into ONE function produced a single
/// ~32MB / ~400K-instruction basic block, which is catastrophically expensive
/// for LLVM to optimize as one function (~36 min). Every op here is independent
/// — each writes to its own global or a
/// runtime registry; no SSA value flows between ops — so splitting at op
/// boundaries is safe and order-preserving (chunks run in sequence, ops in order
/// within a chunk).
#[derive(Default)]
struct InitChunks {
    ops: usize,
    current: Option<usize>,
    names: Vec<String>,
}

pub(super) struct InitChunker<'a> {
    llmod: &'a mut LlModule,
    base_name: String,
    ops_per_chunk: usize,
    // Literal infrastructure may run before cyclic dependencies. Declared
    // class metadata retains its existing module-body initialization boundary.
    pub(super) literals: bool,
    phases: [InitChunks; 2],
}

impl<'a> InitChunker<'a> {
    pub(super) fn new(llmod: &'a mut LlModule, base_name: String, ops_per_chunk: usize) -> Self {
        Self {
            llmod,
            base_name,
            ops_per_chunk: ops_per_chunk.max(1),
            literals: true,
            phases: Default::default(),
        }
    }

    pub(super) fn module(&mut self) -> &mut LlModule {
        self.llmod
    }

    pub(super) fn roll_if_full(&mut self) {
        let phase = usize::from(!self.literals);
        let state = &mut self.phases[phase];
        if state.current.is_none() || state.ops >= self.ops_per_chunk {
            if let Some(current) = state.current {
                self.llmod
                    .function_mut(current)
                    .unwrap()
                    .block_mut(0)
                    .unwrap()
                    .ret_void();
            }
            let name = format!(
                "{}_{}_chunk{}",
                self.base_name,
                if self.literals { "literal" } else { "class" },
                state.names.len()
            );
            self.llmod
                .define_function(&name, VOID, vec![])
                .create_block("entry");
            state.current = Some(self.llmod.function_count() - 1);
            state.names.push(name);
            state.ops = 0;
        }
    }

    pub(super) fn current_block(&mut self) -> &mut LlBlock {
        let state = &mut self.phases[usize::from(!self.literals)];
        state.ops += 1;
        self.llmod
            .function_mut(state.current.unwrap())
            .unwrap()
            .block_mut(0)
            .unwrap()
    }

    pub(super) fn finish(self) -> [Vec<String>; 2] {
        self.phases.map(|state| {
            if let Some(current) = state.current {
                self.llmod
                    .function_mut(current)
                    .unwrap()
                    .block_mut(0)
                    .unwrap()
                    .ret_void();
            }
            state.names
        })
    }
}
