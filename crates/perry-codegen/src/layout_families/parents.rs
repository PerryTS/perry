//! Program-wide class heritage. These are compilation facts only. A resolved
//! parent describes a proposed layout; runtime identity checks admit births.

use super::*;
use perry_hir::ir::{Export, ImportSpecifier};

pub(super) struct Parents<'a> {
    modules: &'a [(&'a str, &'a HirModule)],
    paths: HashMap<&'a str, usize>,
    owners: HashMap<u32, usize>,
    bindings: Vec<HashMap<u32, String>>,
    writes: HashMap<(usize, String), u32>,
    reexports: &'a BTreeMap<(String, String), String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(path: &str, source: &str, offset: u32) -> HirModule {
        let ast = perry_parser::parse_typescript(source, path).expect("parse fixture");
        let mut hir = perry_hir::lower_module(&ast, path, path).expect("lower fixture");
        for c in &mut hir.classes {
            c.id += offset;
            if let Some(parent) = &mut c.extends {
                *parent += offset;
            }
        }
        for import in &mut hir.imports {
            import.resolved_path = Some(import.source.clone());
        }
        hir
    }

    fn child<'a>(families: &'a [RawFamily], hir: &HirModule, name: &str) -> &'a RawFamily {
        let id = hir
            .classes
            .iter()
            .find(|c| c.name == name)
            .expect("class")
            .id;
        families
            .iter()
            .find(|f| f.class_id == id)
            .expect("family admitted")
    }

    #[test]
    fn named_and_default_imports_include_the_exporters_constructor() {
        let base = module("/base", "export default class Base { constructor() { this.base = 1; } } export { Base as Named };", 100);
        let leaf = module("/leaf", "import Parent, {Named} from '/base'; class Leaf extends Parent { constructor() { super(); this.leaf = 2; } } class Other extends Named { constructor() { super(); this.other = 3; } }", 200);
        let modules = [("/base", &base), ("/leaf", &leaf)];
        let families = program_families(&modules, &BTreeMap::new(), &mut BTreeMap::new());
        let f = child(&families, &leaf, "Leaf");
        assert_eq!(f.keys, vec!["base", "leaf"]);
        assert_eq!(f.parent, base.classes[0].id);
        assert_eq!(child(&families, &leaf, "Other").keys, vec!["base", "other"]);
    }

    #[test]
    fn reexports_and_import_cycles_resolve_without_global_name_lookup() {
        let base = module(
            "/base",
            "import '/leaf'; export class Base { constructor() { this.base = 1; } }",
            100,
        );
        let bridge = module("/bridge", "export {Base as Parent} from '/base';", 200);
        let leaf = module("/leaf", "import {Parent} from '/bridge'; class Leaf extends Parent { constructor() { super(); this.leaf = 2; } }", 300);
        let unrelated = module(
            "/unrelated",
            "export class Base { constructor() { this.wrong = 9; } }",
            400,
        );
        let modules = [
            ("/base", &base),
            ("/bridge", &bridge),
            ("/leaf", &leaf),
            ("/unrelated", &unrelated),
        ];
        let reexports = [(("/bridge".into(), "/base".into()), "/base".into())].into();
        let families = program_families(&modules, &reexports, &mut BTreeMap::new());
        assert_eq!(child(&families, &leaf, "Leaf").keys, vec!["base", "leaf"]);
    }

    #[test]
    fn cjs_requires_closed_single_export_and_counts_cross_module_writes() {
        // Original syntax is attached before CJS wrapping. This fixture's
        // lowered module stands in for that wrapper, including its marker.
        let mut base = module("/base", "import {createRequire as __perry_cjs_create_require} from 'module'; class Base { constructor() { this.base = 1; } }", 100);
        base.class_family_syntax = Some(perry_hir::ir::ClassFamilySyntax {
            exports: [("Base".into(), "Base".into())].into(),
            writes: vec![(None, "Base".into())],
            ..Default::default()
        });
        let mut leaf = module("/leaf", "import req from '/base'; class Leaf extends req { constructor() { super(); this.leaf = 2; } }", 200);
        leaf.classes[0].extends_name = Some("ImportedBase".into());
        leaf.class_family_syntax = Some(perry_hir::ir::ClassFamilySyntax {
            parents: [("Leaf".into(), ("/base".into(), "Base".into()))].into(),
            ..Default::default()
        });
        let modules = [("/base", &base), ("/leaf", &leaf)];
        let families = program_families(&modules, &BTreeMap::new(), &mut BTreeMap::new());
        assert_eq!(child(&families, &leaf, "Leaf").keys, vec!["base", "leaf"]);
        leaf.class_family_syntax
            .as_mut()
            .unwrap()
            .writes
            .push((Some("/base".into()), "Base".into()));
        let mut refused = BTreeMap::new();
        let modules = [("/base", &base), ("/leaf", &leaf)];
        let families = program_families(&modules, &BTreeMap::new(), &mut refused);
        assert!(!families.iter().any(|f| f.class_id == leaf.classes[0].id));
        assert_eq!(refused["cjs-export-reassigned"], 1);
        leaf.class_family_syntax.as_mut().unwrap().writes[0]
            .1
            .clear();
        let modules = [("/base", &base), ("/leaf", &leaf)];
        let mut refused = BTreeMap::new();
        program_families(&modules, &BTreeMap::new(), &mut refused);
        assert_eq!(refused["cjs-export-whole-or-escape"], 1);
    }

    #[test]
    fn ambiguous_star_exports_are_refused() {
        let a = module(
            "/a",
            "export class Base { constructor() { this.a = 1; } }",
            100,
        );
        let b = module(
            "/b",
            "export class Base { constructor() { this.b = 2; } }",
            200,
        );
        let bridge = module("/bridge", "export * from '/a'; export * from '/b';", 300);
        let leaf = module("/leaf", "import {Base} from '/bridge'; class Leaf extends Base { constructor() { super(); this.leaf = 3; } }", 400);
        let modules = [
            ("/a", &a),
            ("/b", &b),
            ("/bridge", &bridge),
            ("/leaf", &leaf),
        ];
        let edges = [
            (("/bridge".into(), "/a".into()), "/a".into()),
            (("/bridge".into(), "/b".into()), "/b".into()),
        ]
        .into();
        let mut refused = BTreeMap::new();
        let families = program_families(&modules, &edges, &mut refused);
        assert!(!families.iter().any(|f| f.class_id == leaf.classes[0].id));
        assert_eq!(refused["ambiguous-export"], 1);
    }

    #[test]
    fn an_extends_call_is_refused_by_reason() {
        let hir = module("/leaf", "class Base {} function choose() { return Base; } class Leaf extends choose() { constructor() { super(); this.leaf = 2; } }", 100);
        let mut refused = BTreeMap::new();
        program_families(&[("/leaf", &hir)], &BTreeMap::new(), &mut refused);
        assert_eq!(refused["dynamic-heritage"], 1);
    }
}

impl<'a> Parents<'a> {
    pub fn new(
        modules: &'a [(&'a str, &'a HirModule)],
        reexports: &'a BTreeMap<(String, String), String>,
    ) -> Self {
        let mut graph = Self {
            modules,
            paths: modules
                .iter()
                .enumerate()
                .map(|(i, (p, _))| (*p, i))
                .collect(),
            owners: HashMap::new(),
            bindings: modules.iter().map(|(_, m)| class_bindings(m)).collect(),
            writes: HashMap::new(),
            reexports,
        };
        let mut open_require = false;
        for (i, (_, m)) in modules.iter().enumerate() {
            for c in &m.classes {
                graph.owners.insert(c.id, i);
            }
            if let Some(syntax) = &m.class_family_syntax {
                for (source, key) in &syntax.writes {
                    if source.as_deref() == Some("") {
                        open_require = true;
                        continue;
                    }
                    let target = match source {
                        None => Some(i),
                        Some(source) => graph.target(i, source).ok(),
                    };
                    if let Some(target) = target {
                        *graph.writes.entry((target, key.clone())).or_default() += 1;
                    }
                }
            }
        }
        if open_require {
            for (i, (_, m)) in modules.iter().enumerate() {
                if m.is_commonjs_wrap() {
                    graph.writes.insert((i, String::new()), 1);
                }
            }
        }
        graph
    }

    fn target(&self, module: usize, source: &str) -> Result<usize, &'static str> {
        let path = self.modules[module]
            .1
            .imports
            .iter()
            .find(|i| i.source == source && !i.type_only && !i.runtime_erased)
            .and_then(|i| i.resolved_path.as_deref())
            .or_else(|| {
                self.reexports
                    .get(&(self.modules[module].0.to_string(), source.to_string()))
                    .map(String::as_str)
            });
        path.and_then(|p| self.paths.get(p))
            .copied()
            .ok_or("unresolved-import")
    }

    fn class(&self, module: usize, name: &str) -> Result<&'a Class, &'static str> {
        let mut classes = self.modules[module]
            .1
            .classes
            .iter()
            .filter(|c| c.name == name);
        let class = classes.next().ok_or("unresolved-class")?;
        if classes.next().is_some() {
            return Err("ambiguous-class");
        }
        Ok(class)
    }

    fn exported(
        &self,
        module: usize,
        name: &str,
        seen: &mut Vec<(usize, String)>,
    ) -> Result<&'a Class, &'static str> {
        if seen.len() >= 64 || seen.iter().any(|(m, n)| *m == module && n == name) {
            return Err("import-cycle");
        }
        seen.push((module, name.to_string()));
        let result = self.exported_inner(module, name, seen);
        seen.pop();
        result
    }

    fn exported_inner(
        &self,
        module: usize,
        name: &str,
        seen: &mut Vec<(usize, String)>,
    ) -> Result<&'a Class, &'static str> {
        let hir = self.modules[module].1;
        if hir.is_commonjs_wrap() {
            if self.writes.contains_key(&(module, String::new())) {
                return Err("cjs-export-whole-or-escape");
            }
            if self.writes.get(&(module, name.to_string())).copied() != Some(1) {
                return Err("cjs-export-reassigned");
            }
            let class = hir
                .class_family_syntax
                .as_ref()
                .and_then(|s| s.exports.get(name))
                .ok_or("cjs-export-not-top-level-class")?;
            return self.class(module, class);
        }
        // Explicit exports take precedence over star exports.
        for export in &hir.exports {
            match export {
                Export::Named { local, exported } if exported == name => {
                    return self.named(module, local, seen);
                }
                Export::ReExport {
                    source,
                    imported,
                    exported,
                } if exported == name => {
                    return self.exported(self.target(module, source)?, imported, seen);
                }
                _ => {}
            }
        }
        if let Ok(c) = self.class(module, name) {
            if c.is_exported {
                return Ok(c);
            }
        }
        let mut found: Option<&Class> = None;
        if name != "default" {
            for export in &hir.exports {
                if let Export::ExportAll { source } = export {
                    match self.exported(self.target(module, source)?, name, seen) {
                        Ok(c) => {
                            if found.is_some_and(|old| old.id != c.id) {
                                return Err("ambiguous-export");
                            }
                            found = Some(c);
                        }
                        Err("ambiguous-export") => return Err("ambiguous-export"),
                        Err(_) => {}
                    }
                }
            }
        }
        found.ok_or("unresolved-export")
    }

    fn named(
        &self,
        module: usize,
        name: &str,
        seen: &mut Vec<(usize, String)>,
    ) -> Result<&'a Class, &'static str> {
        let hir = self.modules[module].1;
        for import in &hir.imports {
            if import.type_only || import.runtime_erased || import.is_adopted_require {
                continue;
            }
            for spec in &import.specifiers {
                let exported = match spec {
                    ImportSpecifier::Named { imported, local } if local == name => {
                        Some(imported.as_str())
                    }
                    ImportSpecifier::Default { local } if local == name => Some("default"),
                    _ => None,
                };
                if let Some(exported) = exported {
                    return self.exported(self.target(module, &import.source)?, exported, seen);
                }
            }
        }
        self.class(module, name)
    }

    pub fn native(&self, c: &Class) -> Option<(u32, &'static [&'static str])> {
        let module = *self.owners.get(&c.id)?;
        let n = c.extends_name.as_deref()?;
        if !node_event_emitter_heritage(c)
            || self.modules[module].1.classes.iter().any(|c| c.name == n)
        {
            return None;
        }
        Some((
            crate::expr::builtin_parent_reserved_class_id(n)?,
            &crate::expr::EVENT_EMITTER_INIT_KEYS[..],
        ))
    }

    pub fn parent(&self, c: &Class) -> Result<Option<&'a Class>, &'static str> {
        if self.native(c).is_some() {
            return Ok(None);
        }
        let module = self.owners[&c.id];
        let hir = self.modules[module].1;
        if let Some((source, key)) = hir
            .class_family_syntax
            .as_ref()
            .and_then(|s| s.parents.get(&c.name))
        {
            return self
                .exported(self.target(module, source)?, key, &mut Vec::new())
                .map(Some);
        }
        // A CJS require is not an ESM default import. Without an original
        // const/literal binding proof, refuse rather than trusting its wrap.
        if hir.is_commonjs_wrap() && c.extends.is_none() {
            if c.extends_name.is_some() || c.extends_expr.is_some() {
                if !matches!(c.extends_expr.as_deref(), Some(Expr::ClassRef(_)))
                    && !matches!(c.extends_expr.as_deref(), Some(Expr::LocalGet(id)) if self.bindings[module].contains_key(id))
                {
                    return Err("cjs-parent-binding");
                }
            }
        }
        let name = match c.extends_expr.as_deref() {
            Some(Expr::LocalGet(id)) => self.bindings[module]
                .get(id)
                .map(String::as_str)
                .or(c.extends_name.as_deref())
                .ok_or("unresolved-binding")?,
            Some(Expr::ClassRef(n)) => n,
            Some(Expr::ExternFuncRef { name: n, .. }) => c.extends_name.as_deref().unwrap_or(n),
            Some(_) => return Err("dynamic-heritage"),
            None => match c.extends_name.as_deref() {
                Some(n) => n,
                None => {
                    return match c.extends {
                        None => Ok(None),
                        Some(id) => self
                            .owners
                            .get(&id)
                            .and_then(|m| self.modules[*m].1.classes.iter().find(|p| p.id == id))
                            .map(Some)
                            .ok_or("unresolved-class-id"),
                    };
                }
            },
        };
        self.named(module, name, &mut Vec::new()).map(Some)
    }
}
