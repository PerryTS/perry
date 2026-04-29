//! AST-based fixer for automatically correcting compatibility issues
//!
//! This module provides analysis and fix generation for:
//! - `any` types → inferred or `unknown`
//! - Template literals → string concatenation
//! - Other fixable patterns

use perry_diagnostics::{FileId, Span};
use perry_parser::swc_ecma_ast::*;
use perry_parser::Spanned;
use std::collections::HashMap;

/// A fixable issue with its suggested replacement
#[derive(Debug, Clone)]
pub struct FixableIssue {
    /// Source span (byte offsets)
    pub span: Span,
    /// Type of issue
    pub kind: FixableKind,
    /// Original source text
    pub original: String,
    /// Suggested replacement
    pub replacement: String,
    /// Confidence level for auto-applying
    pub confidence: Confidence,
    /// Human-readable message
    pub message: String,
}

/// Types of fixable issues
#[derive(Debug, Clone)]
pub enum FixableKind {
    /// `any` type that should be replaced
    AnyType {
        /// Inferred type if available, otherwise None (will use `unknown`)
        inferred: Option<String>,
    },
    /// Template literal that needs conversion to concatenation
    TemplateLiteral,
}

/// Confidence level for fixes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// Safe to apply automatically
    High,
    /// Apply with --fix, show with --fix-dry-run
    Medium,
    /// Only show as suggestion, never auto-apply
    Low,
}

/// Analyzer that finds fixable issues in source code
pub struct Fixer {
    file_id: FileId,
    source: String,
    issues: Vec<FixableIssue>,
    /// Track variable usages for type inference
    variable_usages: HashMap<String, VariableUsage>,
}

/// Tracks how a variable is used (for type inference)
#[derive(Debug, Default, Clone)]
struct VariableUsage {
    /// Properties accessed on this variable
    property_accesses: Vec<String>,
    /// Whether it's indexed like an array
    is_indexed: bool,
    /// Whether it's called as a function
    is_called: bool,
    /// Number of arguments when called
    call_arg_count: Option<usize>,
}

impl Fixer {
    /// Analyze a module for fixable issues
    pub fn analyze(module: &Module, file_id: FileId, source: &str) -> Vec<FixableIssue> {
        let mut fixer = Fixer {
            file_id,
            source: source.to_string(),
            issues: Vec::new(),
            variable_usages: HashMap::new(),
        };

        // First pass: collect variable usages for type inference
        fixer.collect_usages(module);

        // Second pass: find fixable issues
        fixer.find_issues(module);

        fixer.issues
    }

    /// Collect variable usages for type inference
    fn collect_usages(&mut self, module: &Module) {
        for item in &module.body {
            if let ModuleItem::Stmt(stmt) = item {
                self.collect_usages_in_stmt(stmt);
            }
        }
    }

    fn collect_usages_in_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(expr_stmt) => self.collect_usages_in_expr(&expr_stmt.expr),
            Stmt::Decl(decl) => self.collect_usages_in_decl(decl),
            Stmt::Block(block) => {
                for stmt in &block.stmts {
                    self.collect_usages_in_stmt(stmt);
                }
            }
            Stmt::If(if_stmt) => {
                self.collect_usages_in_expr(&if_stmt.test);
                self.collect_usages_in_stmt(&if_stmt.cons);
                if let Some(alt) = &if_stmt.alt {
                    self.collect_usages_in_stmt(alt);
                }
            }
            Stmt::While(while_stmt) => {
                self.collect_usages_in_expr(&while_stmt.test);
                self.collect_usages_in_stmt(&while_stmt.body);
            }
            Stmt::For(for_stmt) => {
                if let Some(test) = &for_stmt.test {
                    self.collect_usages_in_expr(test);
                }
                self.collect_usages_in_stmt(&for_stmt.body);
            }
            Stmt::Return(ret) => {
                if let Some(arg) = &ret.arg {
                    self.collect_usages_in_expr(arg);
                }
            }
            _ => {}
        }
    }

    fn collect_usages_in_decl(&mut self, decl: &Decl) {
        if let Decl::Var(var_decl) = decl {
            for decl in &var_decl.decls {
                if let Some(init) = &decl.init {
                    self.collect_usages_in_expr(init);
                }
            }
        }
    }

    fn collect_usages_in_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Member(member) => {
                // Track property access
                if let Expr::Ident(obj_ident) = member.obj.as_ref() {
                    let var_name = obj_ident.sym.to_string();
                    if let MemberProp::Ident(prop) = &member.prop {
                        self.variable_usages
                            .entry(var_name)
                            .or_default()
                            .property_accesses
                            .push(prop.sym.to_string());
                    }
                }
                self.collect_usages_in_expr(&member.obj);
            }
            Expr::Call(call) => {
                // Track function calls
                if let Callee::Expr(callee) = &call.callee {
                    if let Expr::Ident(ident) = callee.as_ref() {
                        let usage = self
                            .variable_usages
                            .entry(ident.sym.to_string())
                            .or_default();
                        usage.is_called = true;
                        usage.call_arg_count = Some(call.args.len());
                    }
                    self.collect_usages_in_expr(callee);
                }
                for arg in &call.args {
                    self.collect_usages_in_expr(&arg.expr);
                }
            }
            Expr::Bin(bin) => {
                self.collect_usages_in_expr(&bin.left);
                self.collect_usages_in_expr(&bin.right);
            }
            Expr::Assign(assign) => {
                self.collect_usages_in_expr(&assign.right);
            }
            Expr::Array(arr) => {
                for elem in arr.elems.iter().flatten() {
                    self.collect_usages_in_expr(&elem.expr);
                }
            }
            Expr::Object(obj) => {
                for prop in &obj.props {
                    if let PropOrSpread::Prop(prop) = prop {
                        if let Prop::KeyValue(kv) = prop.as_ref() {
                            self.collect_usages_in_expr(&kv.value);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Find all fixable issues in the module
    fn find_issues(&mut self, module: &Module) {
        for item in &module.body {
            match item {
                ModuleItem::Stmt(stmt) => self.find_issues_in_stmt(stmt),
                ModuleItem::ModuleDecl(decl) => self.find_issues_in_module_decl(decl),
            }
        }
    }

    fn find_issues_in_module_decl(&mut self, decl: &ModuleDecl) {
        match decl {
            ModuleDecl::ExportDecl(export) => self.find_issues_in_decl(&export.decl),
            ModuleDecl::ExportDefaultExpr(export) => self.find_issues_in_expr(&export.expr),
            _ => {}
        }
    }

    fn find_issues_in_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(expr_stmt) => self.find_issues_in_expr(&expr_stmt.expr),
            Stmt::Decl(decl) => self.find_issues_in_decl(decl),
            Stmt::Block(block) => {
                for stmt in &block.stmts {
                    self.find_issues_in_stmt(stmt);
                }
            }
            Stmt::If(if_stmt) => {
                self.find_issues_in_expr(&if_stmt.test);
                self.find_issues_in_stmt(&if_stmt.cons);
                if let Some(alt) = &if_stmt.alt {
                    self.find_issues_in_stmt(alt);
                }
            }
            Stmt::While(while_stmt) => {
                self.find_issues_in_expr(&while_stmt.test);
                self.find_issues_in_stmt(&while_stmt.body);
            }
            Stmt::For(for_stmt) => {
                if let Some(test) = &for_stmt.test {
                    self.find_issues_in_expr(test);
                }
                self.find_issues_in_stmt(&for_stmt.body);
            }
            Stmt::Return(ret) => {
                if let Some(arg) = &ret.arg {
                    self.find_issues_in_expr(arg);
                }
            }
            _ => {}
        }
    }

    fn find_issues_in_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Var(var_decl) => {
                for decl in &var_decl.decls {
                    // Check type annotation for `any`
                    if let Some(type_ann) = &decl.name.as_ident().and_then(|i| i.type_ann.as_ref())
                    {
                        self.find_any_in_type_ann(
                            type_ann,
                            decl.name.as_ident().map(|i| i.sym.to_string()),
                        );
                    }
                    // Check initializer for template literals
                    if let Some(init) = &decl.init {
                        self.find_issues_in_expr(init);
                    }
                }
            }
            Decl::Fn(fn_decl) => {
                self.find_issues_in_function(&fn_decl.function);
            }
            Decl::Class(class_decl) => {
                self.find_issues_in_class(&class_decl.class);
            }
            Decl::TsInterface(iface) => {
                for member in &iface.body.body {
                    self.find_any_in_ts_type_element(member);
                }
            }
            Decl::TsTypeAlias(alias) => {
                self.find_any_in_ts_type(&alias.type_ann, None);
            }
            _ => {}
        }
    }

    fn find_issues_in_function(&mut self, func: &Function) {
        // Check parameters
        for param in &func.params {
            if let Pat::Ident(ident) = &param.pat {
                if let Some(type_ann) = &ident.type_ann {
                    self.find_any_in_type_ann(type_ann, Some(ident.sym.to_string()));
                }
            }
        }
        // Check return type
        if let Some(return_type) = &func.return_type {
            self.find_any_in_type_ann(return_type, None);
        }
        // Check body
        if let Some(body) = &func.body {
            for stmt in &body.stmts {
                self.find_issues_in_stmt(stmt);
            }
        }
    }

    fn find_issues_in_class(&mut self, class: &Class) {
        for member in &class.body {
            match member {
                ClassMember::Method(method) => {
                    self.find_issues_in_function(&method.function);
                }
                ClassMember::ClassProp(prop) => {
                    if let Some(type_ann) = &prop.type_ann {
                        self.find_any_in_type_ann(type_ann, None);
                    }
                    if let Some(value) = &prop.value {
                        self.find_issues_in_expr(value);
                    }
                }
                ClassMember::Constructor(ctor) => {
                    for param in &ctor.params {
                        match param {
                            ParamOrTsParamProp::Param(p) => {
                                if let Pat::Ident(ident) = &p.pat {
                                    if let Some(type_ann) = &ident.type_ann {
                                        self.find_any_in_type_ann(
                                            type_ann,
                                            Some(ident.sym.to_string()),
                                        );
                                    }
                                }
                            }
                            ParamOrTsParamProp::TsParamProp(prop) => {
                                if let TsParamPropParam::Ident(ident) = &prop.param {
                                    if let Some(type_ann) = &ident.type_ann {
                                        self.find_any_in_type_ann(
                                            type_ann,
                                            Some(ident.sym.to_string()),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    if let Some(body) = &ctor.body {
                        for stmt in &body.stmts {
                            self.find_issues_in_stmt(stmt);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn find_any_in_ts_type_element(&mut self, element: &TsTypeElement) {
        match element {
            TsTypeElement::TsPropertySignature(prop) => {
                if let Some(type_ann) = &prop.type_ann {
                    self.find_any_in_type_ann(type_ann, None);
                }
            }
            TsTypeElement::TsMethodSignature(method) => {
                if let Some(type_ann) = &method.type_ann {
                    self.find_any_in_type_ann(type_ann, None);
                }
                for param in &method.params {
                    if let TsFnParam::Ident(ident) = param {
                        if let Some(type_ann) = &ident.type_ann {
                            self.find_any_in_type_ann(type_ann, None);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn find_any_in_type_ann(&mut self, type_ann: &TsTypeAnn, var_name: Option<String>) {
        self.find_any_in_ts_type(&type_ann.type_ann, var_name);
    }

    fn find_any_in_ts_type(&mut self, ts_type: &TsType, var_name: Option<String>) {
        match ts_type {
            TsType::TsKeywordType(kw) if kw.kind == TsKeywordTypeKind::TsAnyKeyword => {
                // Found an `any` type!
                let span = Span::new(self.file_id, kw.span.lo.0, kw.span.hi.0);
                let original = self.get_source_text(&span);

                // Try to infer type from usage
                let inferred = var_name
                    .as_ref()
                    .and_then(|name| self.infer_type_from_usage(name));

                let has_inferred = inferred.is_some();
                let replacement = inferred.clone().unwrap_or_else(|| "unknown".to_string());

                self.issues.push(FixableIssue {
                    span,
                    kind: FixableKind::AnyType { inferred },
                    original,
                    replacement: replacement.clone(),
                    confidence: if has_inferred {
                        Confidence::Medium
                    } else {
                        Confidence::High
                    },
                    message: format!("Replace 'any' with '{}'", replacement),
                });
            }
            TsType::TsArrayType(arr) => {
                self.find_any_in_ts_type(&arr.elem_type, var_name);
            }
            TsType::TsUnionOrIntersectionType(union_or_intersection) => match union_or_intersection
            {
                TsUnionOrIntersectionType::TsUnionType(union) => {
                    for t in &union.types {
                        self.find_any_in_ts_type(t, var_name.clone());
                    }
                }
                TsUnionOrIntersectionType::TsIntersectionType(intersection) => {
                    for t in &intersection.types {
                        self.find_any_in_ts_type(t, var_name.clone());
                    }
                }
            },
            TsType::TsFnOrConstructorType(fn_type) => match fn_type {
                TsFnOrConstructorType::TsFnType(fn_t) => {
                    for param in &fn_t.params {
                        if let TsFnParam::Ident(ident) = param {
                            if let Some(type_ann) = &ident.type_ann {
                                self.find_any_in_type_ann(type_ann, None);
                            }
                        }
                    }
                    self.find_any_in_type_ann(&fn_t.type_ann, None);
                }
                TsFnOrConstructorType::TsConstructorType(ctor) => {
                    for param in &ctor.params {
                        if let TsFnParam::Ident(ident) = param {
                            if let Some(type_ann) = &ident.type_ann {
                                self.find_any_in_type_ann(type_ann, None);
                            }
                        }
                    }
                    self.find_any_in_type_ann(&ctor.type_ann, None);
                }
            },
            TsType::TsTypeLit(lit) => {
                for member in &lit.members {
                    self.find_any_in_ts_type_element(member);
                }
            }
            TsType::TsParenthesizedType(paren) => {
                self.find_any_in_ts_type(&paren.type_ann, var_name);
            }
            _ => {}
        }
    }

    fn find_issues_in_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Tpl(tpl) => {
                // Found a template literal - create fix
                let span = Span::new(self.file_id, tpl.span.lo.0, tpl.span.hi.0);
                let original = self.get_source_text(&span);
                let replacement = self.convert_template_literal(tpl);

                self.issues.push(FixableIssue {
                    span,
                    kind: FixableKind::TemplateLiteral,
                    original,
                    replacement: replacement.clone(),
                    confidence: Confidence::High,
                    message: format!("Convert template literal to: {}", replacement),
                });
            }
            Expr::Call(call) => {
                if let Callee::Expr(callee) = &call.callee {
                    self.find_issues_in_expr(callee);
                }
                for arg in &call.args {
                    self.find_issues_in_expr(&arg.expr);
                }
            }
            Expr::Member(member) => {
                self.find_issues_in_expr(&member.obj);
            }
            Expr::Bin(bin) => {
                self.find_issues_in_expr(&bin.left);
                self.find_issues_in_expr(&bin.right);
            }
            Expr::Assign(assign) => {
                self.find_issues_in_expr(&assign.right);
            }
            Expr::Arrow(arrow) => {
                // Check parameters
                for param in &arrow.params {
                    if let Pat::Ident(ident) = param {
                        if let Some(type_ann) = &ident.type_ann {
                            self.find_any_in_type_ann(type_ann, Some(ident.sym.to_string()));
                        }
                    }
                }
                // Check return type
                if let Some(return_type) = &arrow.return_type {
                    self.find_any_in_type_ann(return_type, None);
                }
                // Check body
                match arrow.body.as_ref() {
                    BlockStmtOrExpr::BlockStmt(block) => {
                        for stmt in &block.stmts {
                            self.find_issues_in_stmt(stmt);
                        }
                    }
                    BlockStmtOrExpr::Expr(expr) => {
                        self.find_issues_in_expr(expr);
                    }
                }
            }
            Expr::Array(arr) => {
                for elem in arr.elems.iter().flatten() {
                    self.find_issues_in_expr(&elem.expr);
                }
            }
            Expr::Object(obj) => {
                for prop in &obj.props {
                    if let PropOrSpread::Prop(prop) = prop {
                        if let Prop::KeyValue(kv) = prop.as_ref() {
                            self.find_issues_in_expr(&kv.value);
                        }
                    }
                }
            }
            Expr::Cond(cond) => {
                self.find_issues_in_expr(&cond.test);
                self.find_issues_in_expr(&cond.cons);
                self.find_issues_in_expr(&cond.alt);
            }
            Expr::Paren(paren) => {
                self.find_issues_in_expr(&paren.expr);
            }
            _ => {}
        }
    }

    /// Infer a type from how a variable is used
    fn infer_type_from_usage(&self, var_name: &str) -> Option<String> {
        let usage = self.variable_usages.get(var_name)?;

        if !usage.property_accesses.is_empty() {
            // Variable is used as an object - infer shape
            let props: Vec<String> = usage
                .property_accesses
                .iter()
                .map(|prop| format!("{}: unknown", prop))
                .collect();
            // Deduplicate
            let mut unique_props: Vec<String> = props.clone();
            unique_props.sort();
            unique_props.dedup();
            Some(format!("{{ {} }}", unique_props.join("; ")))
        } else if usage.is_indexed {
            Some("unknown[]".to_string())
        } else if usage.is_called {
            let arg_count = usage.call_arg_count.unwrap_or(0);
            let args: Vec<&str> = (0..arg_count).map(|_| "unknown").collect();
            Some(format!("({}) => unknown", args.join(", ")))
        } else {
            None
        }
    }

    /// Get source text for a span
    /// Note: SWC spans use BytePos which starts at 1, not 0
    fn get_source_text(&self, span: &Span) -> String {
        // SWC BytePos starts at 1, so we need to subtract 1 for 0-indexed string slicing
        let start = span.start.saturating_sub(1) as usize;
        let end = span.end.saturating_sub(1) as usize;
        if start <= self.source.len() && end <= self.source.len() && start <= end {
            self.source[start..end].to_string()
        } else {
            String::new()
        }
    }

    /// Convert a template literal to string concatenation
    fn convert_template_literal(&self, tpl: &Tpl) -> String {
        let mut parts: Vec<String> = Vec::new();

        for (i, quasi) in tpl.quasis.iter().enumerate() {
            // Get the cooked string (processed escape sequences)
            if let Some(cooked) = &quasi.cooked {
                let s = cooked.as_str().unwrap_or("");
                if !s.is_empty() {
                    // Escape single quotes and wrap in single quotes
                    let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
                    parts.push(format!("'{}'", escaped));
                }
            }

            // Add the expression if there is one
            if i < tpl.exprs.len() {
                let expr = &tpl.exprs[i];
                let expr_span = Span::new(self.file_id, expr.span().lo.0, expr.span().hi.0);
                let expr_text = self.get_source_text(&expr_span);

                // Wrap complex expressions in parentheses
                let needs_parens = self.expr_needs_parens(expr);
                if needs_parens {
                    parts.push(format!("({})", expr_text));
                } else {
                    parts.push(expr_text);
                }
            }
        }

        if parts.is_empty() {
            "''".to_string()
        } else {
            parts.join(" + ")
        }
    }

    /// Check if an expression needs parentheses when used in concatenation
    fn expr_needs_parens(&self, expr: &Expr) -> bool {
        matches!(
            expr,
            Expr::Bin(_) | Expr::Cond(_) | Expr::Assign(_) | Expr::Seq(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_diagnostics::SourceCache;

    fn analyze_code(source: &str) -> Vec<FixableIssue> {
        let mut cache = SourceCache::new();
        let result = perry_parser::parse_typescript_with_cache(source, "test.ts", &mut cache)
            .expect("Parse failed");
        Fixer::analyze(&result.module, result.file_id, source)
    }

    #[test]
    fn test_any_type_detection() {
        let issues = analyze_code("let x: any = 5;");
        assert_eq!(issues.len(), 1);
        assert!(matches!(issues[0].kind, FixableKind::AnyType { .. }));
        assert_eq!(issues[0].replacement, "unknown");
    }

    #[test]
    fn test_template_literal_simple() {
        let issues = analyze_code("const s = `hello`;");
        assert_eq!(issues.len(), 1);
        assert!(matches!(issues[0].kind, FixableKind::TemplateLiteral));
        assert_eq!(issues[0].replacement, "'hello'");
    }

    #[test]
    fn test_template_literal_with_expr() {
        let issues = analyze_code("const s = `hello ${name}`;");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].replacement, "'hello ' + name");
    }

    #[test]
    fn test_template_literal_complex() {
        let issues = analyze_code("const s = `a ${x + 1} b`;");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].replacement, "'a ' + (x + 1) + ' b'");
    }
}
