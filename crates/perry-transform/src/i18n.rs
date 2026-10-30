//! i18n transform pass
//!
//! Walks all HIR modules and replaces string literals in UI component calls
//! with I18nString nodes that reference the global i18n string table.

use perry_hir::{Expr, Module, Stmt};
use std::collections::{BTreeMap, HashMap, HashSet};

/// UI widget constructors whose first argument is a localizable string.
const LOCALIZABLE_WIDGETS: &[&str] = &[
    "Button",
    "Text",
    "Label",
    "TextField",
    "TextArea",
    "Tab",
    "NavigationTitle",
    "SectionHeader",
    "SecureField",
    "Alert",
];

/// Configuration for the i18n system, parsed from perry.toml.
#[derive(Debug, Clone)]
pub struct I18nConfig {
    pub locales: Vec<String>,
    pub default_locale: String,
    pub dynamic: bool,
    /// Currency codes per locale: locale → ISO 4217 code (e.g., "en" → "USD")
    pub currencies: std::collections::HashMap<String, String>,
}

/// A diagnostic emitted during the i18n transform.
#[derive(Debug, Clone)]
pub struct I18nDiagnostic {
    pub severity: I18nSeverity,
    pub message: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum I18nSeverity {
    Warning,
    Error,
}

/// The i18n string table built by the transform pass.
/// Contains all translations laid out for efficient codegen.
#[derive(Debug, Clone)]
pub struct I18nStringTable {
    /// Ordered list of unique i18n keys (index = string_idx)
    pub keys: Vec<String>,
    /// For each [locale_idx][string_idx], the translated string.
    /// Layout: translations[locale_idx * key_count + string_idx]
    pub translations: Vec<String>,
    /// Number of configured locales
    pub locale_count: usize,
    /// Ordered locale codes (same order as i18n_config.locales)
    pub locale_codes: Vec<String>,
    /// Index of the default locale in the locales array
    pub default_locale_idx: usize,
    /// Diagnostics emitted during the transform
    pub diagnostics: Vec<I18nDiagnostic>,
}

/// Apply the i18n transform to all modules.
///
/// Walks every HIR module, finds string literals in UI widget constructor calls,
/// and replaces them with `Expr::I18nString` referencing the global string table.
pub fn apply_i18n(
    modules: &mut BTreeMap<std::path::PathBuf, Module>,
    config: &I18nConfig,
    translations: &BTreeMap<String, BTreeMap<String, String>>,
) -> I18nStringTable {
    let mut diagnostics = Vec::new();
    let mut key_to_idx: HashMap<String, u32> = HashMap::new();
    let mut keys: Vec<String> = Vec::new();

    let default_locale_idx = config
        .locales
        .iter()
        .position(|l| l == &config.default_locale)
        .unwrap_or(0);

    // First pass: collect all localizable string keys from all modules
    for (_, module) in modules.iter() {
        collect_keys_from_stmts(&module.init, &mut key_to_idx, &mut keys);
        for func in &module.functions {
            collect_keys_from_stmts(&func.body, &mut key_to_idx, &mut keys);
        }
        for class in &module.classes {
            collect_class_function_keys(class, &mut key_to_idx, &mut keys);
        }
    }

    // --- Plural detection ---
    // Scan locale files for keys with plural suffixes (.zero, .one, .two, .few, .many, .other)
    // and register them as additional string table entries.
    let plural_suffixes: &[(&str, u8)] = &[
        (".zero", 0),
        (".one", 1),
        (".two", 2),
        (".few", 3),
        (".many", 4),
        (".other", 5),
    ];
    // plural_info: base_key → Vec<(category, string_idx)>
    let mut plural_info: HashMap<String, Vec<(u8, u32)>> = HashMap::new();
    // Track which plural suffixed keys are used (for dead-key suppression)
    let mut plural_suffixed_keys: HashSet<String> = HashSet::new();

    // For each key used in code, check if locale files have plural variants
    let code_keys: Vec<String> = keys.clone();
    for base_key in &code_keys {
        let mut forms: Vec<(u8, u32)> = Vec::new();
        for &(suffix, category) in plural_suffixes {
            let suffixed_key = format!("{}{}", base_key, suffix);
            // Check if any locale has this suffixed key
            let has_suffix = translations.values().any(|t| t.contains_key(&suffixed_key));
            if has_suffix {
                let idx = register_key(&suffixed_key, &mut key_to_idx, &mut keys);
                forms.push((category, idx));
                plural_suffixed_keys.insert(suffixed_key);
            }
        }
        if !forms.is_empty() {
            plural_info.insert(base_key.clone(), forms);
        }
    }

    // Detect the plural parameter name from {param} in the key
    // e.g., "You have {count} items" → plural_param = "count"
    let mut plural_param_map: HashMap<String, String> = HashMap::new();
    for base_key in plural_info.keys() {
        // Find the first {param} in the key
        if let Some(open) = base_key.find('{') {
            if let Some(close) = base_key[open..].find('}') {
                let param = &base_key[open + 1..open + close];
                plural_param_map.insert(base_key.clone(), param.to_string());
            }
        }
    }

    // Build the translation table
    let key_count = keys.len();
    let mut table_translations = Vec::with_capacity(config.locales.len() * key_count);

    for locale in &config.locales {
        let locale_translations = translations.get(locale);
        for key in &keys {
            let translated = locale_translations
                .and_then(|t| t.get(key))
                .cloned()
                .unwrap_or_else(|| {
                    if locale != &config.default_locale {
                        let fallback = translations
                            .get(&config.default_locale)
                            .and_then(|t| t.get(key))
                            .cloned()
                            .unwrap_or_else(|| key.clone());
                        diagnostics.push(I18nDiagnostic {
                            severity: I18nSeverity::Warning,
                            message: format!(
                                "Missing translation for key \"{}\" in locale \"{}\"",
                                key, locale
                            ),
                            key: key.clone(),
                        });
                        fallback
                    } else {
                        key.clone()
                    }
                });
            table_translations.push(translated);
        }
    }

    // Check for dead keys (in locale files but never used in code)
    let used_keys: HashSet<&str> = keys.iter().map(|k| k.as_str()).collect();
    for (locale, locale_map) in translations {
        for key in locale_map.keys() {
            // Skip plural-suffixed keys that belong to a used base key
            if plural_suffixed_keys.contains(key) {
                continue;
            }
            if !used_keys.contains(key.as_str()) {
                diagnostics.push(I18nDiagnostic {
                    severity: I18nSeverity::Warning,
                    message: format!("Unused i18n key \"{}\" in locale \"{}\"", key, locale),
                    key: key.clone(),
                });
            }
        }
    }

    // Closed-shape object literals are rewritten during HIR lowering — i.e.
    // BEFORE this transform ever sees them — into `Expr::New` on a synthesized
    // `__AnonShape_<hash>` class whose constructor takes the field values as
    // positional args in field-declaration order (see perry-hir
    // lower/context.rs::synthesize_anon_shape_class). So a params literal like
    // `t("Hi {name}", { name })` reaches extract_params as New, not
    // Expr::Object. Map each anon-shape class to its ordered field names so
    // extract_params can recover the `name → value` pairs from that form.
    let mut anon_shape_fields: HashMap<String, Vec<String>> = HashMap::new();
    for (_, module) in modules.iter() {
        for class in &module.classes {
            if class.name.starts_with("__AnonShape_") {
                anon_shape_fields
                    .entry(class.name.clone())
                    .or_insert_with(|| class.fields.iter().map(|f| f.name.clone()).collect());
            }
        }
    }

    // Second pass: replace Expr::String with Expr::I18nString in all modules
    for (_, module) in modules.iter_mut() {
        replace_in_stmts(
            &mut module.init,
            &key_to_idx,
            &plural_info,
            &plural_param_map,
            &anon_shape_fields,
        );
        for func in &mut module.functions {
            replace_in_stmts(
                &mut func.body,
                &key_to_idx,
                &plural_info,
                &plural_param_map,
                &anon_shape_fields,
            );
        }
        for class in &mut module.classes {
            replace_class_function_keys(
                class,
                &key_to_idx,
                &plural_info,
                &plural_param_map,
                &anon_shape_fields,
            );
        }
    }

    I18nStringTable {
        keys,
        translations: table_translations,
        locale_count: config.locales.len(),
        locale_codes: config.locales.clone(),
        default_locale_idx,
        diagnostics,
    }
}

fn collect_class_function_keys(
    class: &perry_hir::ir::Class,
    key_to_idx: &mut HashMap<String, u32>,
    keys: &mut Vec<String>,
) {
    for function in &class.methods {
        collect_keys_from_stmts(&function.body, key_to_idx, keys);
    }
    if let Some(constructor) = &class.constructor {
        collect_keys_from_stmts(&constructor.body, key_to_idx, keys);
    }
    for function in &class.static_methods {
        collect_keys_from_stmts(&function.body, key_to_idx, keys);
    }
    for (_, function) in class.getters.iter().chain(&class.setters) {
        collect_keys_from_stmts(&function.body, key_to_idx, keys);
    }
    for member in &class.computed_members {
        collect_keys_from_stmts(&member.function.body, key_to_idx, keys);
    }
}

fn replace_class_function_keys(
    class: &mut perry_hir::ir::Class,
    key_to_idx: &HashMap<String, u32>,
    plural_info: &HashMap<String, Vec<(u8, u32)>>,
    plural_param_map: &HashMap<String, String>,
    anon_shapes: &HashMap<String, Vec<String>>,
) {
    for function in &mut class.methods {
        replace_in_stmts(
            &mut function.body,
            key_to_idx,
            plural_info,
            plural_param_map,
            anon_shapes,
        );
    }
    if let Some(constructor) = &mut class.constructor {
        replace_in_stmts(
            &mut constructor.body,
            key_to_idx,
            plural_info,
            plural_param_map,
            anon_shapes,
        );
    }
    for function in &mut class.static_methods {
        replace_in_stmts(
            &mut function.body,
            key_to_idx,
            plural_info,
            plural_param_map,
            anon_shapes,
        );
    }
    for (_, function) in class.getters.iter_mut().chain(&mut class.setters) {
        replace_in_stmts(
            &mut function.body,
            key_to_idx,
            plural_info,
            plural_param_map,
            anon_shapes,
        );
    }
    for member in &mut class.computed_members {
        replace_in_stmts(
            &mut member.function.body,
            key_to_idx,
            plural_info,
            plural_param_map,
            anon_shapes,
        );
    }
}

// --- Key collection (first pass) ---

fn collect_keys_from_stmts(
    stmts: &[Stmt],
    key_to_idx: &mut HashMap<String, u32>,
    keys: &mut Vec<String>,
) {
    for stmt in stmts {
        collect_keys_from_stmt(stmt, key_to_idx, keys);
    }
}

fn collect_keys_from_stmt(
    stmt: &Stmt,
    key_to_idx: &mut HashMap<String, u32>,
    keys: &mut Vec<String>,
) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Return(Some(expr))
        | Stmt::Throw(expr) => {
            collect_keys_from_expr(expr, key_to_idx, keys);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_keys_from_expr(condition, key_to_idx, keys);
            collect_keys_from_stmts(then_branch, key_to_idx, keys);
            if let Some(else_b) = else_branch {
                collect_keys_from_stmts(else_b, key_to_idx, keys);
            }
        }
        Stmt::While { condition, body } => {
            collect_keys_from_expr(condition, key_to_idx, keys);
            collect_keys_from_stmts(body, key_to_idx, keys);
        }
        Stmt::DoWhile { body, condition } => {
            collect_keys_from_stmts(body, key_to_idx, keys);
            collect_keys_from_expr(condition, key_to_idx, keys);
        }
        Stmt::Labeled { body, .. } => collect_keys_from_stmt(body, key_to_idx, keys),
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_keys_from_stmt(init_stmt, key_to_idx, keys);
            }
            if let Some(cond) = condition {
                collect_keys_from_expr(cond, key_to_idx, keys);
            }
            if let Some(upd) = update {
                collect_keys_from_expr(upd, key_to_idx, keys);
            }
            collect_keys_from_stmts(body, key_to_idx, keys);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            collect_keys_from_stmts(body, key_to_idx, keys);
            if let Some(catch_clause) = catch {
                collect_keys_from_stmts(&catch_clause.body, key_to_idx, keys);
            }
            if let Some(finally_body) = finally {
                collect_keys_from_stmts(finally_body, key_to_idx, keys);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_keys_from_expr(discriminant, key_to_idx, keys);
            for case in cases {
                if let Some(test) = &case.test {
                    collect_keys_from_expr(test, key_to_idx, keys);
                }
                collect_keys_from_stmts(&case.body, key_to_idx, keys);
            }
        }
        Stmt::Let { init: None, .. }
        | Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

fn collect_keys_from_expr(
    expr: &Expr,
    key_to_idx: &mut HashMap<String, u32>,
    keys: &mut Vec<String>,
) {
    match expr {
        // The target: UI widget calls with string literal as first arg
        Expr::NativeMethodCall {
            module,
            method,
            object,
            args,
            ..
        } => {
            if module == "perry/ui" && object.is_none() && is_localizable_widget(method) {
                if let Some(Expr::String(key)) = args.first() {
                    register_key(key, key_to_idx, keys);
                }
            }
            if module == "perry/i18n" && method == "t" {
                if let Some(Expr::String(key)) = args.first() {
                    register_key(key, key_to_idx, keys);
                }
            }
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                collect_keys_from_expr(child, key_to_idx, keys);
            });
        }
        Expr::Closure { body, .. } => {
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                collect_keys_from_expr(child, key_to_idx, keys);
            });
            collect_keys_from_stmts(body, key_to_idx, keys);
        }
        _ => perry_hir::walker::walk_expr_children(expr, &mut |child| {
            collect_keys_from_expr(child, key_to_idx, keys);
        }),
    }
}

fn register_key(key: &str, key_to_idx: &mut HashMap<String, u32>, keys: &mut Vec<String>) -> u32 {
    if let Some(&idx) = key_to_idx.get(key) {
        return idx;
    }
    let idx = keys.len() as u32;
    key_to_idx.insert(key.to_string(), idx);
    keys.push(key.to_string());
    idx
}

fn is_localizable_widget(method: &str) -> bool {
    LOCALIZABLE_WIDGETS.contains(&method)
}

/// Extract named parameters from a params-object argument.
/// `Text("Hello, {name}!", { name: user.name })` → [("name", Expr::PropertyGet(user, "name"))]
///
/// Two lowered forms reach us:
/// - `Expr::Object` — open-shape literals that survived HIR lowering;
/// - `Expr::New { class_name: "__AnonShape_…", args }` — closed-shape
///   literals, rewritten by HIR lowering into a constructor call whose args
///   are the field values in field-declaration order. `anon_shapes` maps the
///   class name back to those field names.
fn extract_params(
    expr: &Expr,
    anon_shapes: &HashMap<String, Vec<String>>,
) -> Vec<(String, Box<Expr>)> {
    match expr {
        Expr::Object(fields) => fields
            .iter()
            .map(|(name, value)| (name.clone(), Box::new(value.clone())))
            .collect(),
        Expr::New {
            class_name, args, ..
        } => match anon_shapes.get(class_name) {
            Some(field_names) => field_names
                .iter()
                .zip(args.iter())
                .map(|(name, value)| (name.clone(), Box::new(value.clone())))
                .collect(),
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

// --- Replacement (second pass) ---

fn replace_in_stmts(
    stmts: &mut [Stmt],
    key_to_idx: &HashMap<String, u32>,
    plural_info: &HashMap<String, Vec<(u8, u32)>>,
    plural_param_map: &HashMap<String, String>,
    anon_shapes: &HashMap<String, Vec<String>>,
) {
    for stmt in stmts.iter_mut() {
        replace_in_stmt(stmt, key_to_idx, plural_info, plural_param_map, anon_shapes);
    }
}

fn replace_in_stmt(
    stmt: &mut Stmt,
    key_to_idx: &HashMap<String, u32>,
    plural_info: &HashMap<String, Vec<(u8, u32)>>,
    plural_param_map: &HashMap<String, String>,
    anon_shapes: &HashMap<String, Vec<String>>,
) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Return(Some(expr))
        | Stmt::Throw(expr) => {
            replace_in_expr(expr, key_to_idx, plural_info, plural_param_map, anon_shapes);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            replace_in_expr(
                condition,
                key_to_idx,
                plural_info,
                plural_param_map,
                anon_shapes,
            );
            replace_in_stmts(
                then_branch,
                key_to_idx,
                plural_info,
                plural_param_map,
                anon_shapes,
            );
            if let Some(else_b) = else_branch {
                replace_in_stmts(
                    else_b,
                    key_to_idx,
                    plural_info,
                    plural_param_map,
                    anon_shapes,
                );
            }
        }
        Stmt::While { condition, body } => {
            replace_in_expr(
                condition,
                key_to_idx,
                plural_info,
                plural_param_map,
                anon_shapes,
            );
            replace_in_stmts(body, key_to_idx, plural_info, plural_param_map, anon_shapes);
        }
        Stmt::DoWhile { body, condition } => {
            replace_in_stmts(body, key_to_idx, plural_info, plural_param_map, anon_shapes);
            replace_in_expr(
                condition,
                key_to_idx,
                plural_info,
                plural_param_map,
                anon_shapes,
            );
        }
        Stmt::Labeled { body, .. } => {
            replace_in_stmt(body, key_to_idx, plural_info, plural_param_map, anon_shapes)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                replace_in_stmt(
                    init_stmt,
                    key_to_idx,
                    plural_info,
                    plural_param_map,
                    anon_shapes,
                );
            }
            if let Some(cond) = condition {
                replace_in_expr(cond, key_to_idx, plural_info, plural_param_map, anon_shapes);
            }
            if let Some(upd) = update {
                replace_in_expr(upd, key_to_idx, plural_info, plural_param_map, anon_shapes);
            }
            replace_in_stmts(body, key_to_idx, plural_info, plural_param_map, anon_shapes);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            replace_in_stmts(body, key_to_idx, plural_info, plural_param_map, anon_shapes);
            if let Some(catch_clause) = catch {
                replace_in_stmts(
                    &mut catch_clause.body,
                    key_to_idx,
                    plural_info,
                    plural_param_map,
                    anon_shapes,
                );
            }
            if let Some(finally_body) = finally {
                replace_in_stmts(
                    finally_body,
                    key_to_idx,
                    plural_info,
                    plural_param_map,
                    anon_shapes,
                );
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            replace_in_expr(
                discriminant,
                key_to_idx,
                plural_info,
                plural_param_map,
                anon_shapes,
            );
            for case in cases {
                if let Some(test) = &mut case.test {
                    replace_in_expr(test, key_to_idx, plural_info, plural_param_map, anon_shapes);
                }
                replace_in_stmts(
                    &mut case.body,
                    key_to_idx,
                    plural_info,
                    plural_param_map,
                    anon_shapes,
                );
            }
        }
        Stmt::Let { init: None, .. }
        | Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

fn replace_in_expr(
    expr: &mut Expr,
    key_to_idx: &HashMap<String, u32>,
    plural_info: &HashMap<String, Vec<(u8, u32)>>,
    plural_param_map: &HashMap<String, String>,
    anon_shapes: &HashMap<String, Vec<String>>,
) {
    match expr {
        Expr::NativeMethodCall {
            module,
            method,
            object,
            args,
            ..
        } => {
            let is_localizable_ui =
                module == "perry/ui" && object.is_none() && is_localizable_widget(method);
            let is_i18n_t = module == "perry/i18n" && method == "t";

            if (is_localizable_ui || is_i18n_t) && !args.is_empty() {
                if let Expr::String(key) = &args[0] {
                    if let Some(&idx) = key_to_idx.get(key) {
                        let params = if args.len() > 1 {
                            extract_params(&args[1], anon_shapes)
                        } else {
                            Vec::new()
                        };
                        let forms = plural_info.get(key).cloned().unwrap_or_default();
                        let p_param = plural_param_map.get(key).cloned();
                        args[0] = Expr::I18nString {
                            key: key.clone(),
                            string_idx: idx,
                            params,
                            plural_forms: forms,
                            plural_param: p_param,
                        };
                    }
                }
            }
            perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
                replace_in_expr(
                    child,
                    key_to_idx,
                    plural_info,
                    plural_param_map,
                    anon_shapes,
                );
            });
        }
        Expr::Closure { params, body, .. } => {
            for param in params {
                if let Some(default) = &mut param.default {
                    replace_in_expr(
                        default,
                        key_to_idx,
                        plural_info,
                        plural_param_map,
                        anon_shapes,
                    );
                }
            }
            replace_in_stmts(body, key_to_idx, plural_info, plural_param_map, anon_shapes);
        }
        _ => perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
            replace_in_expr(
                child,
                key_to_idx,
                plural_info,
                plural_param_map,
                anon_shapes,
            );
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(key: &str) -> Expr {
        Expr::NativeMethodCall {
            module: "perry/i18n".into(),
            class_name: None,
            object: None,
            method: "t".into(),
            args: vec![Expr::String(key.into())],
        }
    }

    fn set_exprs() -> Vec<Expr> {
        vec![
            Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "label".into(),
                value: Box::new(t("property")),
                byte_offset: 0,
                byte_end: 0,
            },
            Expr::IndexSet {
                object: Box::new(Expr::This),
                index: Box::new(Expr::String("slot".into())),
                value: Box::new(t("index")),
                byte_offset: 0,
                byte_end: 0,
            },
            Expr::PutValueSet {
                target: Box::new(Expr::This),
                key: Box::new(Expr::String("slot".into())),
                value: Box::new(t("put-value")),
                receiver: Box::new(Expr::This),
                strict: true,
                byte_offset: 0,
                byte_end: 0,
            },
        ]
    }

    fn collect(expr: &Expr) -> Vec<String> {
        let mut map = HashMap::new();
        let mut keys = Vec::new();
        collect_keys_from_expr(expr, &mut map, &mut keys);
        keys
    }

    fn collect_stmts(stmts: &[Stmt]) -> Vec<String> {
        let mut map = HashMap::new();
        let mut keys = Vec::new();
        collect_keys_from_stmts(stmts, &mut map, &mut keys);
        keys
    }

    fn assert_localized(expr: &Expr, expected_key: &str) {
        let Expr::NativeMethodCall { args, .. } = expr else {
            panic!("expected localization call")
        };
        assert!(matches!(
            args.first(),
            Some(Expr::I18nString { key, .. }) if key == expected_key
        ));
    }

    fn class_function(name: &str, key: &str, id: u32) -> perry_hir::Function {
        perry_hir::Function {
            id,
            name: name.into(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: perry_hir::types::Type::Void,
            body: vec![Stmt::Expr(t(key))],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn class_function_key(function: &perry_hir::Function) -> &str {
        let Stmt::Expr(Expr::NativeMethodCall { args, .. }) = &function.body[0] else {
            panic!("expected localization call in function body")
        };
        let Some(Expr::I18nString { key, .. }) = args.first() else {
            panic!("expected localized string in function body")
        };
        key
    }

    #[test]
    fn apply_i18n_visits_all_class_function_containers() {
        let class = perry_hir::ir::Class {
            id: 1,
            name: "Example".into(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: Vec::new(),
            constructor: None,
            methods: vec![class_function("method", "ordinary-method", 1)],
            getters: vec![("value".into(), class_function("get value", "getter", 2))],
            setters: vec![("value".into(), class_function("set value", "setter", 3))],
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            static_fields: Vec::new(),
            static_methods: vec![class_function("staticMethod", "static-method", 4)],
            computed_members: vec![perry_hir::ir::ClassComputedMember {
                key_expr: Expr::String("computedName".into()),
                function: class_function("computed", "computed-member", 5),
                is_static: false,
                kind: perry_hir::ir::ClassComputedMemberKind::Method,
                source_order: 0,
            }],
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
            alloc_width_hint: 0,
            specialized_from: None,
        };
        let expected = [
            "ordinary-method",
            "static-method",
            "getter",
            "setter",
            "computed-member",
        ];
        let mut module = Module::new("test");
        module.classes.push(class.clone());
        let translations = BTreeMap::from([(
            "en".into(),
            expected
                .iter()
                .map(|key| ((*key).into(), format!("translated {key}")))
                .collect(),
        )]);
        let config = I18nConfig {
            locales: vec!["en".into()],
            default_locale: "en".into(),
            dynamic: false,
            currencies: HashMap::new(),
        };

        let mut modules = BTreeMap::from([(std::path::PathBuf::from("test"), module)]);
        let table = apply_i18n(&mut modules, &config, &translations);

        assert_eq!(table.keys, expected);
        let transformed = &modules.values().next().unwrap().classes[0];
        assert_eq!(
            class_function_key(&transformed.methods[0]),
            "ordinary-method"
        );
        assert_eq!(
            class_function_key(&transformed.static_methods[0]),
            "static-method"
        );
        assert_eq!(class_function_key(&transformed.getters[0].1), "getter");
        assert_eq!(class_function_key(&transformed.setters[0].1), "setter");
        assert_eq!(
            class_function_key(&transformed.computed_members[0].function),
            "computed-member"
        );
    }

    #[test]
    fn collects_and_replaces_localization_calls_in_all_setter_values() {
        let mut expr = Expr::Sequence(set_exprs());
        let keys = collect(&expr);
        assert_eq!(keys, ["property", "index", "put-value"]);

        let map = keys
            .iter()
            .enumerate()
            .map(|(i, k)| (k.clone(), i as u32))
            .collect::<HashMap<_, _>>();
        replace_in_expr(
            &mut expr,
            &map,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let mut replaced = Vec::new();
        fn visit(expr: &Expr, found: &mut Vec<String>) {
            if let Expr::I18nString { key, .. } = expr {
                found.push(key.clone());
            }
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                visit(child, found);
            });
        }
        visit(&expr, &mut replaced);
        assert_eq!(replaced, keys);
    }

    #[test]
    fn setter_collection_deduplicates_keys_in_first_seen_order() {
        let expr = Expr::Sequence(vec![
            Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "a".into(),
                value: Box::new(t("repeat")),
                byte_offset: 0,
                byte_end: 0,
            },
            Expr::IndexSet {
                object: Box::new(Expr::This),
                index: Box::new(Expr::String("b".into())),
                value: Box::new(t("next")),
                byte_offset: 0,
                byte_end: 0,
            },
            Expr::PutValueSet {
                target: Box::new(Expr::This),
                key: Box::new(Expr::String("c".into())),
                value: Box::new(t("repeat")),
                receiver: Box::new(Expr::This),
                strict: false,
                byte_offset: 0,
                byte_end: 0,
            },
        ]);
        assert_eq!(collect(&expr), ["repeat", "next"]);
    }

    #[test]
    fn traverses_closure_bodies_and_parameter_defaults() {
        let expr = Expr::Closure {
            func_id: 0,
            params: vec![perry_hir::Param {
                id: 0,
                name: "arg".into(),
                ty: perry_hir::types::Type::String,
                default: Some(t("default")),
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: perry_hir::types::Type::Void,
            body: vec![Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "label".into(),
                value: Box::new(t("body")),
                byte_offset: 0,
                byte_end: 0,
            })],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: false,
        };
        assert_eq!(collect(&expr), ["default", "body"]);
    }

    #[test]
    fn traverses_do_while_and_nested_labels_in_source_order_and_replaces_calls() {
        let mut stmts = vec![
            Stmt::DoWhile {
                body: vec![Stmt::Expr(t("body"))],
                condition: t("condition"),
            },
            Stmt::Labeled {
                label: "outer".into(),
                body: Box::new(Stmt::Labeled {
                    label: "inner".into(),
                    body: Box::new(Stmt::DoWhile {
                        body: vec![Stmt::Expr(t("nested-body"))],
                        condition: t("nested-condition"),
                    }),
                }),
            },
        ];
        let keys = collect_stmts(&stmts);
        assert_eq!(
            keys,
            ["body", "condition", "nested-body", "nested-condition"]
        );

        let key_to_idx = keys
            .iter()
            .enumerate()
            .map(|(i, key)| (key.clone(), i as u32))
            .collect::<HashMap<_, _>>();
        replace_in_stmts(
            &mut stmts,
            &key_to_idx,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        let Stmt::DoWhile { body, condition } = &stmts[0] else {
            panic!("expected do-while")
        };
        let Stmt::Expr(body_expr) = &body[0] else {
            panic!("expected do-while body expression")
        };
        assert_localized(body_expr, "body");
        assert_localized(condition, "condition");

        let Stmt::Labeled { body, .. } = &stmts[1] else {
            panic!("expected outer label")
        };
        let Stmt::Labeled { body, .. } = body.as_ref() else {
            panic!("expected inner label")
        };
        let Stmt::DoWhile { body, condition } = body.as_ref() else {
            panic!("expected nested do-while")
        };
        let Stmt::Expr(nested_body) = &body[0] else {
            panic!("expected nested body expression")
        };
        assert_localized(nested_body, "nested-body");
        assert_localized(condition, "nested-condition");
    }
}
