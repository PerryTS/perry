//! Resolve standalone constructor ABIs once in each defining module's scope.
//! The pipeline runs this after preparing every import route and before either
//! object-cache lookup or LLVM emission (#10258).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::ctor_arity::{context_free_ctor_param_count, UNRESOLVED_PARENT_FWD_ARITY};
use super::opts::CompileOptions;

type Symbol = (String, String);

enum Contract {
    Params(usize),
    Parent(Symbol, usize),
}

/// Populate both producer signatures and consumer declarations from one graph
/// of constructor contracts. Symbols use the canonical defining prefix/name;
/// local aliases and namespace keys only participate in scope lookup.
///
/// Resolving imports first is essential: following a foreign class's bare
/// `extends_name` in the consumer can select an unrelated same-named parent.
/// A runtime heritage with no class binding keeps the existing 8-slot band;
/// a real parent constructor keeps its exact arity, including zero.
pub fn resolve_constructor_contracts<'a>(
    modules: impl IntoIterator<Item = (String, &'a perry_hir::Module, &'a mut CompileOptions)>,
) {
    let mut modules: Vec<_> = modules.into_iter().collect();
    let mut contracts = BTreeMap::new();
    for (prefix, module, opts) in &modules {
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
        for imported in &opts.imported_classes {
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
            contracts.insert((prefix.clone(), class.name.clone()), contract);
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

    let mut resolved = BTreeMap::new();
    for symbol in contracts.keys() {
        resolve(symbol, &contracts, &mut resolved, &mut BTreeSet::new());
    }
    for (prefix, module, opts) in &mut modules {
        opts.constructor_param_counts = module
            .classes
            .iter()
            .map(|class| {
                let count = resolved[&(prefix.clone(), class.name.clone())];
                (class.name.clone(), count)
            })
            .collect();
        for imported in &mut opts.imported_classes {
            if let Some(count) =
                resolved.get(&(imported.source_prefix.clone(), imported.name.clone()))
            {
                imported.constructor_param_count = *count;
            }
        }
    }
}
