//! Resolve standalone constructor ABIs once in each defining module's scope.
//! Collect only constructor edges, then resolve before object-cache lookup or
//! LLVM emission. Per-module import metadata can be dropped immediately (#10265).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::ctor_arity::{context_free_ctor_param_count, UNRESOLVED_PARENT_FWD_ARITY};
use super::opts::{CompileOptions, ImportedClass};

type Symbol = (String, String);

enum Contract {
    Params(usize),
    Parent(Symbol, usize),
}

/// A compact graph of constructor edges in each defining module's scope.
/// No HIR, class bodies, or per-module compile options are retained.
#[derive(Default)]
pub struct ConstructorContracts {
    contracts: BTreeMap<Symbol, Contract>,
}

/// The only graph-wide constructor data needed during parallel codegen.
pub struct ResolvedConstructorContracts {
    arities: BTreeMap<Symbol, usize>,
}

impl ConstructorContracts {
    /// Record a module after resolving its import routes. Aliases and namespace
    /// keys participate only in scope lookup; edges use canonical prefix/name.
    /// The caller can immediately discard the module's temporary import metadata.
    pub fn record(
        &mut self,
        prefix: &str,
        module: &perry_hir::Module,
        imported_classes: &[ImportedClass],
    ) {
        let mut locals: HashMap<_, _> = module
            .classes
            .iter()
            .map(|class| (class.name.as_str(), class))
            .collect();
        for class in &module.classes {
            for alias in &class.aliases {
                locals.entry(alias.as_str()).or_insert(class);
            }
        }
        let mut imports = HashMap::new();
        for imported in imported_classes {
            imports.entry(imported.effective_name()).or_insert(imported);
        }
        for class in &module.classes {
            let contract = if let Some(count) = context_free_ctor_param_count(class) {
                Contract::Params(count)
            } else {
                let mut parent = class.extends_name.as_deref();
                let mut visited = BTreeSet::new();
                let mut contract = Contract::Params(UNRESOLVED_PARENT_FWD_ARITY);
                while let Some(name) = parent {
                    if !visited.insert(name) {
                        break;
                    }
                    if let Some(local) = locals.get(name) {
                        if let Some(ctor) = &local.constructor {
                            contract = Contract::Params(ctor.params.len());
                            break;
                        }
                        parent = local.extends_name.as_deref();
                    } else if let Some(imported) = imports.get(name) {
                        contract = Contract::Parent(
                            (imported.source_prefix.clone(), imported.name.clone()),
                            imported.constructor_param_count,
                        );
                        break;
                    } else {
                        break;
                    }
                }
                contract
            };
            self.contracts
                .insert((prefix.to_owned(), class.name.clone()), contract);
        }
    }

    /// Consume and drop the unresolved edges before any codegen options are built.
    pub fn resolve(self) -> ResolvedConstructorContracts {
        let mut resolved = BTreeMap::new();
        for symbol in self.contracts.keys() {
            resolve(symbol, &self.contracts, &mut resolved, &mut BTreeSet::new());
        }
        ResolvedConstructorContracts { arities: resolved }
    }
}

fn resolve(
    symbol: &Symbol,
    contracts: &BTreeMap<Symbol, Contract>,
    resolved: &mut BTreeMap<Symbol, usize>,
    visiting: &mut BTreeSet<Symbol>,
) -> usize {
    if let Some(count) = resolved.get(symbol) {
        return *count;
    }
    if !visiting.insert(symbol.clone()) {
        // Cyclic heritage has no constructor-bearing ancestor. Keep the
        // standalone fallback and, crucially, the same ABI on every edge.
        return UNRESOLVED_PARENT_FWD_ARITY;
    }
    let count = match &contracts[symbol] {
        Contract::Params(count) => *count,
        Contract::Parent(parent, fallback) => {
            if contracts.contains_key(parent) {
                resolve(parent, contracts, resolved, visiting)
            } else {
                // Synthetic/native capabilities have no HIR producer in
                // this graph and already carry their declared signature.
                *fallback
            }
        }
    };
    visiting.remove(symbol);
    resolved.insert(symbol.clone(), count);
    count
}

impl ResolvedConstructorContracts {
    /// Set producer and consumer signatures before object-cache hashing.
    pub fn apply(&self, prefix: &str, module: &perry_hir::Module, opts: &mut CompileOptions) {
        opts.constructor_param_counts = module
            .classes
            .iter()
            .map(|class| {
                let count = self.arities[&(prefix.to_owned(), class.name.clone())];
                (class.name.clone(), count)
            })
            .collect();
        for imported in &mut opts.imported_classes {
            if let Some(count) = self
                .arities
                .get(&(imported.source_prefix.clone(), imported.name.clone()))
            {
                imported.constructor_param_count = *count;
            }
        }
    }
}
