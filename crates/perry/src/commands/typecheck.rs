//! tsgo IPC client for TypeScript type checking
//!
//! Spawns Microsoft's native TypeScript checker (`tsgo --api`) and communicates
//! over stdin/stdout using the msgpack-based IPC protocol.

use anyhow::{anyhow, Result};
use perry_hir::types::Type;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Client for communicating with tsgo's IPC API
pub struct TsGoClient {
    process: Child,
    snapshot: Option<u64>,
    project: Option<String>,
}

/// Type information returned by tsgo
#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// TypeScript TypeFlags bitmask
    pub flags: u32,
    /// String representation of the type (e.g., "string", "number", "User")
    pub type_string: String,
}

mod wire;
use wire::ApiError;

// The preview API has no protocol-version handshake. Only builds whose wire,
// handles and TypeFlags have been verified are safe to use for lowering.
const SUPPORTED_TSGO_VERSION: &str = "7.0.0-dev.20260612.1";

impl TsGoClient {
    pub fn spawn(project_dir: &Path) -> Result<Self> {
        let tsgo_path = find_tsgo_binary(project_dir)?;
        let version = Command::new(&tsgo_path).arg("--version").output()?;
        let version_text = String::from_utf8_lossy(&version.stdout);
        if !version.status.success()
            || version_text.trim().strip_prefix("Version ") != Some(SUPPORTED_TSGO_VERSION)
        {
            return Err(ApiError::UnsupportedVersion(version_text.trim().to_owned()).into());
        }
        let project_dir = project_dir.canonicalize()?;
        let process = Command::new(&tsgo_path)
            .args(["--api", "--cwd"])
            .arg(&project_dir)
            .current_dir(&project_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut client = Self {
            process,
            snapshot: None,
            project: None,
        };
        let init = client.send_request("initialize", &serde_json::Value::Null)?;
        if !init["useCaseSensitiveFileNames"].is_boolean() || !init["currentDirectory"].is_string()
        {
            return Err(ApiError::Protocol("invalid initialize response".into()).into());
        }
        Ok(client)
    }

    fn send_request(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let payload = self.send_request_binary(method, params)?;
        if payload.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_slice(&payload)
            .map_err(|e| ApiError::Protocol(format!("{method}: invalid JSON: {e}")).into())
    }

    fn send_request_binary(&mut self, method: &str, params: &serde_json::Value) -> Result<Vec<u8>> {
        let stdin = self
            .process
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("tsgo stdin closed"))?;
        wire::write_request(stdin, method, &serde_json::to_vec(params)?)?;
        let stdout = self
            .process
            .stdout
            .as_mut()
            .ok_or_else(|| anyhow!("tsgo stdout closed"))?;
        Ok(wire::read_response(stdout, method)?)
    }

    /// Open the configured project and retain its snapshot/project handles.
    pub fn load_project(&mut self, tsconfig: &Path) -> Result<()> {
        let tsconfig = tsconfig.canonicalize()?;
        let result = self.send_request(
            "updateSnapshot",
            &serde_json::json!({"openProject": tsconfig}),
        )?;
        let snapshot = result["snapshot"]
            .as_u64()
            .ok_or_else(|| ApiError::Protocol("missing snapshot handle".into()))?;
        let projects = result["projects"]
            .as_array()
            .ok_or_else(|| ApiError::Protocol("missing projects".into()))?;
        let project = projects
            .iter()
            .find(|p| {
                p["configFileName"]
                    .as_str()
                    .is_some_and(|name| Path::new(name) == tsconfig)
            })
            .and_then(|p| p["id"].as_str())
            .ok_or_else(|| ApiError::Protocol("requested project missing from snapshot".into()))?;
        self.snapshot = Some(snapshot);
        self.project = Some(project.to_owned());
        Ok(())
    }

    fn project_params(&self) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "snapshot": self.snapshot.ok_or_else(|| anyhow!("tsgo project not open"))?,
            "project": self.project.as_ref().ok_or_else(|| anyhow!("tsgo project not open"))?,
        }))
    }

    pub fn diagnostics(&mut self) -> Result<Vec<Diagnostic>> {
        let params = self.project_params()?;
        let mut diagnostics = Vec::new();
        for method in [
            "getConfigFileParsingDiagnostics",
            "getSyntacticDiagnostics",
            "getSemanticDiagnostics",
        ] {
            let result = self.send_request(method, &params)?;
            let mut items: Vec<Diagnostic> = serde_json::from_value(result)
                .map_err(|e| ApiError::Protocol(format!("{method}: {e}")))?;
            diagnostics.append(&mut items);
        }
        diagnostics.sort_by(|a, b| {
            (&a.file_name, a.pos, a.code, &a.text).cmp(&(&b.file_name, b.pos, b.code, &b.text))
        });
        diagnostics.dedup();
        Ok(diagnostics)
    }

    /// Positions in tsgo's API are zero-based UTF-8 byte offsets.
    pub fn get_types_at_positions(
        &mut self,
        file: &str,
        positions: &[u32],
    ) -> Result<Vec<Option<TypeInfo>>> {
        if positions.is_empty() {
            return Ok(Vec::new());
        }
        let mut params = self.project_params()?;
        params["file"] = file.into();
        params["positions"] = serde_json::to_value(positions)?;
        let result = self.send_request("getTypesAtPositions", &params)?;
        let items = result
            .as_array()
            .filter(|a| a.len() == positions.len())
            .ok_or_else(|| ApiError::Protocol("invalid types response length".into()))?;
        let mut types = Vec::with_capacity(items.len());
        for item in items {
            if item.is_null() {
                types.push(None);
                continue;
            }
            let flags = item["flags"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| ApiError::Protocol("missing type flags".into()))?;
            let id = item["id"]
                .as_u64()
                .ok_or_else(|| ApiError::Protocol("missing type handle".into()))?;
            let mut query = self.project_params()?;
            query["type"] = id.into();
            let name = self.send_request("typeToString", &query)?;
            let type_string = name
                .as_str()
                .ok_or_else(|| ApiError::Protocol("invalid typeToString response".into()))?
                .to_owned();
            types.push(Some(TypeInfo { flags, type_string }));
        }
        Ok(types)
    }

    pub fn shutdown(&mut self) {
        drop(self.process.stdin.take());
        // Do not wait indefinitely on a broken or incompatible checker.
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

impl Drop for TsGoClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Native API diagnostics use UTF-8 byte offsets and flattened message text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub pos: usize,
    pub code: u32,
    pub category: u32,
    pub text: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(file) = &self.file_name {
            let source = std::fs::read_to_string(file).unwrap_or_default();
            let offset = self.pos.min(source.len());
            let prefix = source.get(..offset).unwrap_or("");
            let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
            let column = prefix
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .encode_utf16()
                .count()
                + 1;
            write!(f, "{file}({line},{column}): ")?;
        }
        let category = if self.category == 1 {
            "error"
        } else {
            "warning"
        };
        write!(f, "{category} TS{}: {}", self.code, self.text)
    }
}

/// Convert TypeScript TypeFlags to Perry's Type enum
pub fn ts_flags_to_perry_type(flags: u32, type_string: &str) -> Type {
    // Native-preview TypeFlags (different from the JavaScript tsc enum)
    const STRING: u32 = 32;
    const NUMBER: u32 = 64;
    const BOOLEAN: u32 = 256;
    const BIGINT: u32 = 128;
    const STRING_LITERAL: u32 = 1024;
    const NUMBER_LITERAL: u32 = 2048;
    const BOOLEAN_LITERAL: u32 = 8192;
    const VOID: u32 = 16;
    const UNDEFINED: u32 = 4;
    const NULL: u32 = 8;
    const NEVER: u32 = 262144;
    const OBJECT: u32 = 1048576;
    const UNION: u32 = 134217728;

    // Check primitive flags
    if flags & (STRING | STRING_LITERAL) != 0 {
        return Type::String;
    }
    if flags & (NUMBER | NUMBER_LITERAL) != 0 {
        return Type::Number;
    }
    if flags & (BOOLEAN | BOOLEAN_LITERAL) != 0 {
        return Type::Boolean;
    }
    if flags & (BIGINT | 4096) != 0 {
        return Type::BigInt;
    }
    if flags & (VOID | UNDEFINED) != 0 {
        return Type::Void;
    }
    if flags & NULL != 0 {
        return Type::Null;
    }
    if flags & NEVER != 0 {
        return Type::Never;
    }

    // For object/union types, parse the type_string
    if flags & OBJECT != 0 {
        return parse_type_string(type_string);
    }
    if flags & UNION != 0 {
        return parse_type_string(type_string);
    }

    // Fallback: try parsing the type string directly
    parse_type_string(type_string)
}

/// Parse a TypeScript type string into Perry's Type enum
fn parse_type_string(s: &str) -> Type {
    let s = s.trim();
    match s {
        "string" => Type::String,
        "number" => Type::Number,
        "boolean" => Type::Boolean,
        "bigint" => Type::BigInt,
        "void" => Type::Void,
        "undefined" => Type::Void,
        "null" => Type::Null,
        "never" => Type::Never,
        "any" => Type::Any,
        "unknown" => Type::Unknown,
        "symbol" => Type::Symbol,
        _ => {
            // Check for array types: "string[]", "number[]", etc.
            if let Some(inner) = s.strip_suffix("[]") {
                return Type::Array(Box::new(parse_type_string(inner)));
            }
            // Check for Array<T> generic syntax
            if let Some(inner) = s.strip_prefix("Array<").and_then(|s| s.strip_suffix('>')) {
                return Type::Array(Box::new(parse_type_string(inner)));
            }
            // Check for Promise<T>
            if let Some(inner) = s.strip_prefix("Promise<").and_then(|s| s.strip_suffix('>')) {
                return Type::Promise(Box::new(parse_type_string(inner)));
            }
            // Check for union types: "string | number"
            if s.contains(" | ") {
                let parts: Vec<Type> = s
                    .split(" | ")
                    .map(|p| parse_type_string(p.trim()))
                    .collect();
                if parts.len() > 1 {
                    return Type::Union(parts);
                }
            }
            // Named type (interface, class, etc.)
            if s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                return Type::Named(s.to_string());
            }
            Type::Any
        }
    }
}

/// Find the tsgo binary, checking several locations
fn find_tsgo_binary(project_dir: &Path) -> Result<PathBuf> {
    // 1. Check if tsgo is in PATH
    if let Ok(output) = Command::new("which").arg("tsgo").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    // 2. Check project's node_modules/.bin/tsgo
    let local_bin = project_dir.join("node_modules/.bin/tsgo");
    if local_bin.exists() {
        return Ok(local_bin);
    }

    // 3. Check for npx-style path
    let npx_path = project_dir.join("node_modules/@typescript/native-preview/bin/tsgo");
    if npx_path.exists() {
        return Ok(npx_path);
    }

    Err(anyhow!(
        "tsgo not found. Install with: npm install -g @typescript/native-preview\n\
         or locally: npm install --save-dev @typescript/native-preview"
    ))
}

/// Collect byte positions of all untyped variable declarations from an SWC AST module.
/// Returns a vec of (byte_position, variable_name) pairs.
pub fn collect_untyped_positions(ast_module: &swc_ecma_ast::Module) -> Vec<(u32, String)> {
    let mut positions = Vec::new();

    for item in &ast_module.body {
        collect_untyped_from_item(item, &mut positions);
    }

    positions
}

fn collect_untyped_from_item(item: &swc_ecma_ast::ModuleItem, positions: &mut Vec<(u32, String)>) {
    use swc_ecma_ast as ast;

    match item {
        ast::ModuleItem::Stmt(stmt) => collect_untyped_from_stmt(stmt, positions),
        ast::ModuleItem::ModuleDecl(decl) => {
            if let ast::ModuleDecl::ExportDecl(export) = decl {
                if let ast::Decl::Var(var_decl) = &export.decl {
                    collect_untyped_from_var_decl(var_decl, positions);
                }
                if let ast::Decl::Fn(fn_decl) = &export.decl {
                    // Function without return type annotation
                    if fn_decl.function.return_type.is_none() && fn_decl.function.body.is_some() {
                        positions.push((fn_decl.ident.span.lo.0, fn_decl.ident.sym.to_string()));
                    }
                }
            }
        }
    }
}

fn collect_untyped_from_stmt(stmt: &swc_ecma_ast::Stmt, positions: &mut Vec<(u32, String)>) {
    use swc_ecma_ast as ast;

    match stmt {
        ast::Stmt::Decl(decl) => {
            if let ast::Decl::Var(var_decl) = decl {
                collect_untyped_from_var_decl(var_decl, positions);
            }
            if let ast::Decl::Fn(fn_decl) = decl {
                if fn_decl.function.return_type.is_none() && fn_decl.function.body.is_some() {
                    positions.push((fn_decl.ident.span.lo.0, fn_decl.ident.sym.to_string()));
                }
            }
        }
        ast::Stmt::Block(block) => {
            for s in &block.stmts {
                collect_untyped_from_stmt(s, positions);
            }
        }
        ast::Stmt::If(if_stmt) => {
            collect_untyped_from_stmt(&if_stmt.cons, positions);
            if let Some(alt) = &if_stmt.alt {
                collect_untyped_from_stmt(alt, positions);
            }
        }
        ast::Stmt::While(while_stmt) => {
            collect_untyped_from_stmt(&while_stmt.body, positions);
        }
        ast::Stmt::For(for_stmt) => {
            if let Some(ast::VarDeclOrExpr::VarDecl(var_decl)) = &for_stmt.init {
                collect_untyped_from_var_decl(var_decl, positions);
            }
            collect_untyped_from_stmt(&for_stmt.body, positions);
        }
        ast::Stmt::ForIn(for_in) => {
            collect_untyped_from_stmt(&for_in.body, positions);
        }
        ast::Stmt::ForOf(for_of) => {
            collect_untyped_from_stmt(&for_of.body, positions);
        }
        _ => {}
    }
}

fn collect_untyped_from_var_decl(
    var_decl: &swc_ecma_ast::VarDecl,
    positions: &mut Vec<(u32, String)>,
) {
    use swc_ecma_ast as ast;

    for decl in &var_decl.decls {
        if let ast::Pat::Ident(ident) = &decl.name {
            // Only collect positions where there's no type annotation and there IS an initializer
            if ident.type_ann.is_none() && decl.init.is_some() {
                positions.push((ident.id.span.lo.0, ident.id.sym.to_string()));
            }
        }
    }
}

/// Resolve types for all untyped positions in a file using the tsgo client.
/// Returns a HashMap from byte position to Perry Type.
pub fn resolve_types_for_file(
    client: &mut TsGoClient,
    file_path: &str,
    positions: &[(u32, String)],
) -> Result<HashMap<u32, Type>> {
    if positions.is_empty() {
        return Ok(HashMap::new());
    }

    let byte_positions: Vec<u32> = positions
        .iter()
        .map(|(pos, _)| pos.saturating_sub(1))
        .collect();

    let type_infos = client.get_types_at_positions(file_path, &byte_positions)?;

    let mut resolved = HashMap::new();
    for (i, info) in type_infos.into_iter().enumerate() {
        if let Some(info) = info {
            let ty = ts_flags_to_perry_type(info.flags, &info.type_string);
            if !matches!(ty, Type::Any) {
                resolved.insert(positions[i].0, ty);
            }
        }
    }

    Ok(resolved)
}

/// Find tsconfig.json for a project directory
pub fn find_tsconfig(project_dir: &Path) -> Option<PathBuf> {
    let tsconfig = project_dir.join("tsconfig.json");
    if tsconfig.exists() {
        Some(tsconfig)
    } else {
        // Walk up parent directories
        let mut dir = project_dir.parent();
        while let Some(parent) = dir {
            let tc = parent.join("tsconfig.json");
            if tc.exists() {
                return Some(tc);
            }
            dir = parent.parent();
        }
        None
    }
}

#[cfg(test)]
mod tests;
