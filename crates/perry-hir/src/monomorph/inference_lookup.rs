use super::*;

/// Lightweight function info for inference during update phase
#[derive(Clone)]
pub(crate) struct FuncInfo {
    // #854: populated by `from_module` for completeness; the inference-update
    // consumer keys on the map entry, not this mirrored id. Kept for the
    // self-describing record + future lookups.
    #[allow(dead_code)]
    pub(crate) id: FuncId,
    pub(crate) type_params: Vec<crate::types::TypeParam>,
    pub(crate) params: Vec<Param>,
    pub(crate) return_type: Type,
}

/// Lightweight class info for inference during update phase
#[derive(Clone)]
pub(crate) struct ClassInfo {
    // #854: populated by `from_module`; the map is keyed by class name so this
    // mirrored copy is currently unread. Kept for the self-describing record.
    #[allow(dead_code)]
    pub(crate) name: String,
    pub(crate) type_params: Vec<crate::types::TypeParam>,
    pub(crate) constructor_params: Option<Vec<Param>>,
}

/// Lookup table for type inference during update phase
pub(crate) struct InferenceLookup {
    pub(crate) funcs: HashMap<FuncId, FuncInfo>,
    pub(crate) classes: HashMap<String, ClassInfo>,
}

impl InferenceLookup {
    pub(crate) fn from_module(module: &Module) -> Self {
        let funcs = module
            .functions
            .iter()
            .map(|f| {
                (
                    f.id,
                    FuncInfo {
                        id: f.id,
                        type_params: f.type_params.clone(),
                        params: f.params.clone(),
                        return_type: f.return_type.clone(),
                    },
                )
            })
            .collect();

        let classes = module
            .classes
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    ClassInfo {
                        name: c.name.clone(),
                        type_params: c.type_params.clone(),
                        constructor_params: c.constructor.as_ref().map(|ctor| ctor.params.clone()),
                    },
                )
            })
            .collect();

        Self { funcs, classes }
    }
}
