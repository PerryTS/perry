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
            for method in &class.methods {
                collect_keys_from_stmts(&method.body, &mut key_to_idx, &mut keys);
            }
            if let Some(ctor) = &class.constructor {
                collect_keys_from_stmts(&ctor.body, &mut key_to_idx, &mut keys);
            }
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

    // Second pass: replace Expr::String with Expr::I18nString in all modules
    for (_, module) in modules.iter_mut() {
        replace_in_stmts(
            &mut module.init,
            &key_to_idx,
            &plural_info,
            &plural_param_map,
        );
        for func in &mut module.functions {
            replace_in_stmts(&mut func.body, &key_to_idx, &plural_info, &plural_param_map);
        }
        for class in &mut module.classes {
            for method in &mut class.methods {
                replace_in_stmts(
                    &mut method.body,
                    &key_to_idx,
                    &plural_info,
                    &plural_param_map,
                );
            }
            if let Some(ctor) = &mut class.constructor {
                replace_in_stmts(&mut ctor.body, &key_to_idx, &plural_info, &plural_param_map);
            }
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
        _ => {}
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
            if let Some(obj) = object {
                collect_keys_from_expr(obj, key_to_idx, keys);
            }
            for arg in args {
                collect_keys_from_expr(arg, key_to_idx, keys);
            }
        }
        // Structural expressions — recurse
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_keys_from_expr(left, key_to_idx, keys);
            collect_keys_from_expr(right, key_to_idx, keys);
        }
        Expr::Unary { operand, .. } => {
            collect_keys_from_expr(operand, key_to_idx, keys);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_keys_from_expr(condition, key_to_idx, keys);
            collect_keys_from_expr(then_expr, key_to_idx, keys);
            collect_keys_from_expr(else_expr, key_to_idx, keys);
        }
        Expr::Call { callee, args, .. } => {
            collect_keys_from_expr(callee, key_to_idx, keys);
            for arg in args {
                collect_keys_from_expr(arg, key_to_idx, keys);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_keys_from_expr(callee, key_to_idx, keys);
            for arg in args {
                match arg {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        collect_keys_from_expr(e, key_to_idx, keys);
                    }
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                collect_keys_from_expr(e, key_to_idx, keys);
            }
        }
        Expr::Object(fields) => {
            for (_, val) in fields {
                collect_keys_from_expr(val, key_to_idx, keys);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, val) in parts {
                collect_keys_from_expr(val, key_to_idx, keys);
            }
        }
        Expr::IndexGet { object, index } | Expr::IndexSet { object, index, .. } => {
            collect_keys_from_expr(object, key_to_idx, keys);
            collect_keys_from_expr(index, key_to_idx, keys);
        }
        Expr::PropertyGet { object, .. } | Expr::PropertySet { object, .. } => {
            collect_keys_from_expr(object, key_to_idx, keys);
        }
        Expr::LocalSet(_, expr) | Expr::GlobalSet(_, expr) => {
            collect_keys_from_expr(expr, key_to_idx, keys);
        }
        Expr::Closure { body, .. } => {
            collect_keys_from_stmts(body, key_to_idx, keys);
        }
        Expr::New { args, .. }
        | Expr::NewDynamic { args, .. }
        | Expr::SuperCall(args)
        | Expr::StaticMethodCall { args, .. }
        | Expr::Sequence(args)
        | Expr::MathMin(args)
        | Expr::MathMax(args) => {
            for arg in args {
                collect_keys_from_expr(arg, key_to_idx, keys);
            }
        }
        Expr::Await(expr) | Expr::TypeOf(expr) | Expr::Void(expr) | Expr::Delete(expr) => {
            collect_keys_from_expr(expr, key_to_idx, keys);
        }
        Expr::Yield {
            value: Some(expr), ..
        } => {
            collect_keys_from_expr(expr, key_to_idx, keys);
        }
        Expr::In { property, object } => {
            collect_keys_from_expr(property, key_to_idx, keys);
            collect_keys_from_expr(object, key_to_idx, keys);
        }
        Expr::InstanceOf { expr, .. } => {
            collect_keys_from_expr(expr, key_to_idx, keys);
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args {
                collect_keys_from_expr(arg, key_to_idx, keys);
            }
        }
        // Leaf/domain-specific expressions — no recursion needed
        _ => {}
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

/// Extract named parameters from an object literal expression.
/// `Text("Hello, {name}!", { name: user.name })` → [("name", Expr::PropertyGet(user, "name"))]
fn extract_params(expr: &Expr) -> Vec<(String, Box<Expr>)> {
    match expr {
        Expr::Object(fields) => fields
            .iter()
            .map(|(name, value)| (name.clone(), Box::new(value.clone())))
            .collect(),
        _ => Vec::new(),
    }
}

// --- Replacement (second pass) ---

fn replace_in_stmts(
    stmts: &mut [Stmt],
    key_to_idx: &HashMap<String, u32>,
    plural_info: &HashMap<String, Vec<(u8, u32)>>,
    plural_param_map: &HashMap<String, String>,
) {
    for stmt in stmts.iter_mut() {
        replace_in_stmt(stmt, key_to_idx, plural_info, plural_param_map);
    }
}

fn replace_in_stmt(
    stmt: &mut Stmt,
    key_to_idx: &HashMap<String, u32>,
    plural_info: &HashMap<String, Vec<(u8, u32)>>,
    plural_param_map: &HashMap<String, String>,
) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Return(Some(expr))
        | Stmt::Throw(expr) => {
            replace_in_expr(expr, key_to_idx, plural_info, plural_param_map);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            replace_in_expr(condition, key_to_idx, plural_info, plural_param_map);
            replace_in_stmts(then_branch, key_to_idx, plural_info, plural_param_map);
            if let Some(else_b) = else_branch {
                replace_in_stmts(else_b, key_to_idx, plural_info, plural_param_map);
            }
        }
        Stmt::While { condition, body } => {
            replace_in_expr(condition, key_to_idx, plural_info, plural_param_map);
            replace_in_stmts(body, key_to_idx, plural_info, plural_param_map);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                replace_in_stmt(init_stmt, key_to_idx, plural_info, plural_param_map);
            }
            if let Some(cond) = condition {
                replace_in_expr(cond, key_to_idx, plural_info, plural_param_map);
            }
            if let Some(upd) = update {
                replace_in_expr(upd, key_to_idx, plural_info, plural_param_map);
            }
            replace_in_stmts(body, key_to_idx, plural_info, plural_param_map);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            replace_in_stmts(body, key_to_idx, plural_info, plural_param_map);
            if let Some(catch_clause) = catch {
                replace_in_stmts(
                    &mut catch_clause.body,
                    key_to_idx,
                    plural_info,
                    plural_param_map,
                );
            }
            if let Some(finally_body) = finally {
                replace_in_stmts(finally_body, key_to_idx, plural_info, plural_param_map);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            replace_in_expr(discriminant, key_to_idx, plural_info, plural_param_map);
            for case in cases {
                if let Some(test) = &mut case.test {
                    replace_in_expr(test, key_to_idx, plural_info, plural_param_map);
                }
                replace_in_stmts(&mut case.body, key_to_idx, plural_info, plural_param_map);
            }
        }
        _ => {}
    }
}

fn replace_in_expr(
    expr: &mut Expr,
    key_to_idx: &HashMap<String, u32>,
    plural_info: &HashMap<String, Vec<(u8, u32)>>,
    plural_param_map: &HashMap<String, String>,
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
                        // Extract params from second argument if it's an object literal
                        let params = if args.len() > 1 {
                            extract_params(&args[1])
                        } else {
                            Vec::new()
                        };
                        // Look up plural forms for this key
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
            if let Some(obj) = object {
                replace_in_expr(obj, key_to_idx, plural_info, plural_param_map);
            }
            for arg in args.iter_mut() {
                replace_in_expr(arg, key_to_idx, plural_info, plural_param_map);
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            replace_in_expr(left, key_to_idx, plural_info, plural_param_map);
            replace_in_expr(right, key_to_idx, plural_info, plural_param_map);
        }
        Expr::Unary { operand, .. } => {
            replace_in_expr(operand, key_to_idx, plural_info, plural_param_map);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            replace_in_expr(condition, key_to_idx, plural_info, plural_param_map);
            replace_in_expr(then_expr, key_to_idx, plural_info, plural_param_map);
            replace_in_expr(else_expr, key_to_idx, plural_info, plural_param_map);
        }
        Expr::Call { callee, args, .. } => {
            replace_in_expr(callee, key_to_idx, plural_info, plural_param_map);
            for arg in args.iter_mut() {
                replace_in_expr(arg, key_to_idx, plural_info, plural_param_map);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            replace_in_expr(callee, key_to_idx, plural_info, plural_param_map);
            for arg in args.iter_mut() {
                match arg {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        replace_in_expr(e, key_to_idx, plural_info, plural_param_map);
                    }
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements.iter_mut() {
                replace_in_expr(e, key_to_idx, plural_info, plural_param_map);
            }
        }
        Expr::Object(fields) => {
            for (_, val) in fields.iter_mut() {
                replace_in_expr(val, key_to_idx, plural_info, plural_param_map);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, val) in parts.iter_mut() {
                replace_in_expr(val, key_to_idx, plural_info, plural_param_map);
            }
        }
        Expr::IndexGet { object, index } | Expr::IndexSet { object, index, .. } => {
            replace_in_expr(object, key_to_idx, plural_info, plural_param_map);
            replace_in_expr(index, key_to_idx, plural_info, plural_param_map);
        }
        Expr::PropertyGet { object, .. } | Expr::PropertySet { object, .. } => {
            replace_in_expr(object, key_to_idx, plural_info, plural_param_map);
        }
        Expr::LocalSet(_, expr) | Expr::GlobalSet(_, expr) => {
            replace_in_expr(expr, key_to_idx, plural_info, plural_param_map);
        }
        Expr::Closure { body, .. } => {
            replace_in_stmts(body, key_to_idx, plural_info, plural_param_map);
        }
        Expr::New { args, .. }
        | Expr::NewDynamic { args, .. }
        | Expr::SuperCall(args)
        | Expr::StaticMethodCall { args, .. }
        | Expr::Sequence(args)
        | Expr::MathMin(args)
        | Expr::MathMax(args) => {
            for arg in args.iter_mut() {
                replace_in_expr(arg, key_to_idx, plural_info, plural_param_map);
            }
        }
        Expr::Await(expr) | Expr::TypeOf(expr) | Expr::Void(expr) | Expr::Delete(expr) => {
            replace_in_expr(expr, key_to_idx, plural_info, plural_param_map);
        }
        Expr::Yield {
            value: Some(expr), ..
        } => {
            replace_in_expr(expr, key_to_idx, plural_info, plural_param_map);
        }
        Expr::In { property, object } => {
            replace_in_expr(property, key_to_idx, plural_info, plural_param_map);
            replace_in_expr(object, key_to_idx, plural_info, plural_param_map);
        }
        Expr::InstanceOf { expr, .. } => {
            replace_in_expr(expr, key_to_idx, plural_info, plural_param_map);
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args.iter_mut() {
                replace_in_expr(arg, key_to_idx, plural_info, plural_param_map);
            }
        }
        _ => {}
    }
}
