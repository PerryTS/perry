use super::*;

/// Pre-built index for O(1) lookups into module collections.
/// Built once from the original module state before any specializations are added.
pub(crate) struct ModuleIndex {
    /// Map from function ID to its index in module.functions
    pub(crate) func_by_id: HashMap<FuncId, usize>,
    /// Map from class name to its index in module.classes
    pub(crate) class_by_name: HashMap<String, usize>,
    /// Map from interface name to its index in module.interfaces
    pub(crate) interface_by_name: HashMap<String, usize>,
}

impl ModuleIndex {
    pub(crate) fn new(module: &Module) -> Self {
        let func_by_id: HashMap<FuncId, usize> = module
            .functions
            .iter()
            .enumerate()
            .map(|(i, f)| (f.id, i))
            .collect();

        let class_by_name: HashMap<String, usize> = module
            .classes
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone(), i))
            .collect();

        let interface_by_name: HashMap<String, usize> = module
            .interfaces
            .iter()
            .enumerate()
            .map(|(i, iface)| (iface.name.clone(), i))
            .collect();

        Self {
            func_by_id,
            class_by_name,
            interface_by_name,
        }
    }
}

/// Key for function specialization (func_id, mangled_type_args)
type FuncSpecKey = (FuncId, String);

/// Key for class specialization (class_name, mangled_type_args)
type ClassSpecKey = (String, String);

/// Context for monomorphization pass
pub struct MonomorphizationContext {
    /// Map from (original func_id, mangled_type_args) to specialized func_id
    pub(crate) specialized_funcs: HashMap<FuncSpecKey, FuncId>,
    /// Map from (class_name, mangled_type_args) to specialized class name
    pub(crate) specialized_classes: HashMap<ClassSpecKey, String>,
    /// Queue of functions needing specialization
    pub(crate) func_work_queue: VecDeque<FuncSpecRequest>,
    /// Queue of classes needing specialization
    pub(crate) class_work_queue: VecDeque<ClassSpecRequest>,
    /// Counter for generating unique function IDs
    pub(crate) next_func_id: FuncId,
    /// Counter for generating unique class IDs
    pub(crate) next_class_id: ClassId,
    /// Set of already processed specializations (to avoid duplicates)
    pub(crate) processed_funcs: HashSet<FuncSpecKey>,
    pub(crate) processed_classes: HashSet<ClassSpecKey>,
}

/// Request to specialize a function
#[derive(Debug, Clone)]
pub(crate) struct FuncSpecRequest {
    /// Original function ID
    pub(crate) original_id: FuncId,
    /// Type arguments to substitute
    pub(crate) type_args: Vec<Type>,
    /// New function ID for the specialized version
    pub(crate) new_id: FuncId,
}

/// Request to specialize a class
#[derive(Debug, Clone)]
pub(crate) struct ClassSpecRequest {
    /// Original class name
    pub(crate) original_name: String,
    /// Type arguments to substitute
    pub(crate) type_args: Vec<Type>,
    /// New class name for the specialized version
    // #854: retained for the class-monomorphization spec record; not yet read
    // by the (currently func-only) specialization driver. Mirrors the
    // field-level allow on `FuncSpecRequest::original_name` above.
    #[allow(dead_code)]
    pub(crate) new_name: String,
}

impl MonomorphizationContext {
    pub fn new(module: &Module) -> Self {
        // Find the highest existing func_id and class_id
        let max_func_id = module.functions.iter().map(|f| f.id).max().unwrap_or(0);

        let max_class_id = module.classes.iter().map(|c| c.id).max().unwrap_or(0);

        Self {
            specialized_funcs: HashMap::new(),
            specialized_classes: HashMap::new(),
            func_work_queue: VecDeque::new(),
            class_work_queue: VecDeque::new(),
            next_func_id: max_func_id + 1000, // Leave room for original IDs
            next_class_id: max_class_id + 1000,
            processed_funcs: HashSet::new(),
            processed_classes: HashSet::new(),
        }
    }

    fn fresh_func_id(&mut self) -> FuncId {
        let id = self.next_func_id;
        self.next_func_id += 1;
        id
    }

    pub(crate) fn fresh_class_id(&mut self) -> ClassId {
        let id = self.next_class_id;
        self.next_class_id += 1;
        id
    }

    /// Request specialization of a function with given type arguments
    /// Returns the specialized function's ID
    pub fn request_func_specialization(&mut self, func_id: FuncId, type_args: Vec<Type>) -> FuncId {
        let mangled_args = mangle_type_args(&type_args);
        let key = (func_id, mangled_args);

        if let Some(&specialized_id) = self.specialized_funcs.get(&key) {
            return specialized_id;
        }

        let new_id = self.fresh_func_id();
        self.specialized_funcs.insert(key.clone(), new_id);

        if !self.processed_funcs.contains(&key) {
            self.func_work_queue.push_back(FuncSpecRequest {
                original_id: func_id,
                type_args,
                new_id,
            });
        }

        new_id
    }

    /// Request specialization of a class with given type arguments
    /// Returns the specialized class name
    pub fn request_class_specialization(
        &mut self,
        class_name: &str,
        type_args: Vec<Type>,
    ) -> String {
        let mangled_args = mangle_type_args(&type_args);
        let key = (class_name.to_string(), mangled_args);

        if let Some(specialized_name) = self.specialized_classes.get(&key) {
            return specialized_name.clone();
        }

        let new_name = generate_specialized_name(class_name, &type_args);
        self.specialized_classes
            .insert(key.clone(), new_name.clone());

        if !self.processed_classes.contains(&key) {
            self.class_work_queue.push_back(ClassSpecRequest {
                original_name: class_name.to_string(),
                type_args,
                new_name: new_name.clone(),
            });
        }

        new_name
    }
}
