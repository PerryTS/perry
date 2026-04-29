//! tsgo IPC client for TypeScript type checking
//!
//! Spawns Microsoft's native TypeScript checker (`tsgo --api`) and communicates
//! over stdin/stdout using the msgpack-based IPC protocol.

use anyhow::{anyhow, Result};
use perry_types::Type;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Client for communicating with tsgo's IPC API
pub struct TsGoClient {
    process: Child,
    next_seq: u32,
}

/// Type information returned by tsgo
#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// TypeScript TypeFlags bitmask
    pub flags: u32,
    /// String representation of the type (e.g., "string", "number", "User")
    pub type_string: String,
}

/// msgpack encoding helpers (minimal, no external dependency)
mod msgpack {

    /// Encode a msgpack array header with count elements
    pub fn encode_array_header(buf: &mut Vec<u8>, count: u32) {
        if count <= 15 {
            buf.push(0x90 | count as u8);
        } else if count <= 0xFFFF {
            buf.push(0xdc);
            buf.extend_from_slice(&(count as u16).to_be_bytes());
        } else {
            buf.push(0xdd);
            buf.extend_from_slice(&count.to_be_bytes());
        }
    }

    /// Encode a msgpack string
    pub fn encode_str(buf: &mut Vec<u8>, s: &str) {
        let bytes = s.as_bytes();
        let len = bytes.len();
        if len <= 31 {
            buf.push(0xa0 | len as u8);
        } else if len <= 0xFF {
            buf.push(0xd9);
            buf.push(len as u8);
        } else if len <= 0xFFFF {
            buf.push(0xda);
            buf.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            buf.push(0xdb);
            buf.extend_from_slice(&(len as u32).to_be_bytes());
        }
        buf.extend_from_slice(bytes);
    }

    /// Encode a positive integer
    pub fn encode_uint(buf: &mut Vec<u8>, val: u64) {
        if val <= 127 {
            buf.push(val as u8);
        } else if val <= 0xFF {
            buf.push(0xcc);
            buf.push(val as u8);
        } else if val <= 0xFFFF {
            buf.push(0xcd);
            buf.extend_from_slice(&(val as u16).to_be_bytes());
        } else if val <= 0xFFFFFFFF {
            buf.push(0xce);
            buf.extend_from_slice(&(val as u32).to_be_bytes());
        } else {
            buf.push(0xcf);
            buf.extend_from_slice(&val.to_be_bytes());
        }
    }

    /// Encode nil
    pub fn encode_nil(buf: &mut Vec<u8>) {
        buf.push(0xc0);
    }

    /// Decode a msgpack value, returning the parsed JSON-like value and bytes consumed
    pub fn decode(data: &[u8]) -> Result<(serde_json::Value, usize), String> {
        if data.is_empty() {
            return Err("empty data".to_string());
        }
        let b = data[0];
        match b {
            // nil
            0xc0 => Ok((serde_json::Value::Null, 1)),
            // false
            0xc2 => Ok((serde_json::Value::Bool(false), 1)),
            // true
            0xc3 => Ok((serde_json::Value::Bool(true), 1)),
            // positive fixint (0x00-0x7f)
            0x00..=0x7f => Ok((serde_json::json!(b as u64), 1)),
            // negative fixint (0xe0-0xff)
            0xe0..=0xff => Ok((serde_json::json!(b as i8 as i64), 1)),
            // uint8
            0xcc => {
                if data.len() < 2 {
                    return Err("truncated uint8".to_string());
                }
                Ok((serde_json::json!(data[1] as u64), 2))
            }
            // uint16
            0xcd => {
                if data.len() < 3 {
                    return Err("truncated uint16".to_string());
                }
                let val = u16::from_be_bytes([data[1], data[2]]);
                Ok((serde_json::json!(val as u64), 3))
            }
            // uint32
            0xce => {
                if data.len() < 5 {
                    return Err("truncated uint32".to_string());
                }
                let val = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
                Ok((serde_json::json!(val as u64), 5))
            }
            // uint64
            0xcf => {
                if data.len() < 9 {
                    return Err("truncated uint64".to_string());
                }
                let val = u64::from_be_bytes(data[1..9].try_into().unwrap());
                Ok((serde_json::json!(val), 9))
            }
            // int8
            0xd0 => {
                if data.len() < 2 {
                    return Err("truncated int8".to_string());
                }
                Ok((serde_json::json!(data[1] as i8 as i64), 2))
            }
            // int16
            0xd1 => {
                if data.len() < 3 {
                    return Err("truncated int16".to_string());
                }
                let val = i16::from_be_bytes([data[1], data[2]]);
                Ok((serde_json::json!(val as i64), 3))
            }
            // int32
            0xd2 => {
                if data.len() < 5 {
                    return Err("truncated int32".to_string());
                }
                let val = i32::from_be_bytes([data[1], data[2], data[3], data[4]]);
                Ok((serde_json::json!(val as i64), 5))
            }
            // int64
            0xd3 => {
                if data.len() < 9 {
                    return Err("truncated int64".to_string());
                }
                let val = i64::from_be_bytes(data[1..9].try_into().unwrap());
                Ok((serde_json::json!(val), 9))
            }
            // float32
            0xca => {
                if data.len() < 5 {
                    return Err("truncated float32".to_string());
                }
                let val = f32::from_be_bytes([data[1], data[2], data[3], data[4]]);
                Ok((serde_json::json!(val as f64), 5))
            }
            // float64
            0xcb => {
                if data.len() < 9 {
                    return Err("truncated float64".to_string());
                }
                let val = f64::from_be_bytes(data[1..9].try_into().unwrap());
                Ok((serde_json::json!(val), 9))
            }
            // fixstr (0xa0-0xbf)
            0xa0..=0xbf => {
                let len = (b & 0x1f) as usize;
                if data.len() < 1 + len {
                    return Err("truncated fixstr".to_string());
                }
                let s = std::str::from_utf8(&data[1..1 + len]).map_err(|e| e.to_string())?;
                Ok((serde_json::Value::String(s.to_string()), 1 + len))
            }
            // str8
            0xd9 => {
                if data.len() < 2 {
                    return Err("truncated str8 header".to_string());
                }
                let len = data[1] as usize;
                if data.len() < 2 + len {
                    return Err("truncated str8".to_string());
                }
                let s = std::str::from_utf8(&data[2..2 + len]).map_err(|e| e.to_string())?;
                Ok((serde_json::Value::String(s.to_string()), 2 + len))
            }
            // str16
            0xda => {
                if data.len() < 3 {
                    return Err("truncated str16 header".to_string());
                }
                let len = u16::from_be_bytes([data[1], data[2]]) as usize;
                if data.len() < 3 + len {
                    return Err("truncated str16".to_string());
                }
                let s = std::str::from_utf8(&data[3..3 + len]).map_err(|e| e.to_string())?;
                Ok((serde_json::Value::String(s.to_string()), 3 + len))
            }
            // str32
            0xdb => {
                if data.len() < 5 {
                    return Err("truncated str32 header".to_string());
                }
                let len = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
                if data.len() < 5 + len {
                    return Err("truncated str32".to_string());
                }
                let s = std::str::from_utf8(&data[5..5 + len]).map_err(|e| e.to_string())?;
                Ok((serde_json::Value::String(s.to_string()), 5 + len))
            }
            // bin8
            0xc4 => {
                if data.len() < 2 {
                    return Err("truncated bin8 header".to_string());
                }
                let len = data[1] as usize;
                Ok((serde_json::Value::Null, 2 + len)) // skip binary data
            }
            // bin16
            0xc5 => {
                if data.len() < 3 {
                    return Err("truncated bin16 header".to_string());
                }
                let len = u16::from_be_bytes([data[1], data[2]]) as usize;
                Ok((serde_json::Value::Null, 3 + len))
            }
            // bin32
            0xc6 => {
                if data.len() < 5 {
                    return Err("truncated bin32 header".to_string());
                }
                let len = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
                Ok((serde_json::Value::Null, 5 + len))
            }
            // fixarray (0x90-0x9f)
            0x90..=0x9f => {
                let count = (b & 0x0f) as usize;
                decode_array(data, 1, count)
            }
            // array16
            0xdc => {
                if data.len() < 3 {
                    return Err("truncated array16 header".to_string());
                }
                let count = u16::from_be_bytes([data[1], data[2]]) as usize;
                decode_array(data, 3, count)
            }
            // array32
            0xdd => {
                if data.len() < 5 {
                    return Err("truncated array32 header".to_string());
                }
                let count = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
                decode_array(data, 5, count)
            }
            // fixmap (0x80-0x8f)
            0x80..=0x8f => {
                let count = (b & 0x0f) as usize;
                decode_map(data, 1, count)
            }
            // map16
            0xde => {
                if data.len() < 3 {
                    return Err("truncated map16 header".to_string());
                }
                let count = u16::from_be_bytes([data[1], data[2]]) as usize;
                decode_map(data, 3, count)
            }
            // map32
            0xdf => {
                if data.len() < 5 {
                    return Err("truncated map32 header".to_string());
                }
                let count = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
                decode_map(data, 5, count)
            }
            _ => Err(format!("unsupported msgpack byte: 0x{:02x}", b)),
        }
    }

    fn decode_array(
        data: &[u8],
        start: usize,
        count: usize,
    ) -> Result<(serde_json::Value, usize), String> {
        let mut offset = start;
        let mut arr = Vec::with_capacity(count);
        for _ in 0..count {
            let (val, consumed) = decode(&data[offset..])?;
            arr.push(val);
            offset += consumed;
        }
        Ok((serde_json::Value::Array(arr), offset))
    }

    fn decode_map(
        data: &[u8],
        start: usize,
        count: usize,
    ) -> Result<(serde_json::Value, usize), String> {
        let mut offset = start;
        let mut map = serde_json::Map::new();
        for _ in 0..count {
            let (key, consumed) = decode(&data[offset..])?;
            offset += consumed;
            let (val, consumed) = decode(&data[offset..])?;
            offset += consumed;
            if let serde_json::Value::String(k) = key {
                map.insert(k, val);
            }
        }
        Ok((serde_json::Value::Object(map), offset))
    }

    /// Encode a length-prefixed msgpack message (4-byte big-endian length prefix + payload)
    pub fn encode_message(msg: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + msg.len());
        out.extend_from_slice(&(msg.len() as u32).to_be_bytes());
        out.extend_from_slice(msg);
        out
    }
}

impl TsGoClient {
    /// Spawn a tsgo process in IPC mode.
    /// Looks for `tsgo` in PATH, or falls back to npx @typescript/native-preview.
    pub fn spawn(project_dir: &Path) -> Result<Self> {
        // Try to find tsgo binary
        let tsgo_path = find_tsgo_binary(project_dir)?;

        let process = Command::new(&tsgo_path)
            .arg("--api")
            .current_dir(project_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn tsgo: {}. Install with: npm install -g @typescript/native-preview", e))?;

        Ok(Self {
            process,
            next_seq: 1,
        })
    }

    /// Send an IPC request and read the response.
    /// Protocol: 4-byte big-endian length prefix + msgpack payload.
    /// Payload is a 3-element array: [messageType, methodName, jsonPayload]
    fn send_request(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let seq = self.next_seq;
        self.next_seq += 1;

        // Encode the request: [seq, method, params_json_string]
        let params_json = serde_json::to_string(params)?;
        let mut payload = Vec::new();
        msgpack::encode_array_header(&mut payload, 3);
        msgpack::encode_uint(&mut payload, seq as u64);
        msgpack::encode_str(&mut payload, method);
        msgpack::encode_str(&mut payload, &params_json);

        let message = msgpack::encode_message(&payload);

        // Write to stdin
        let stdin = self
            .process
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("tsgo stdin not available"))?;
        stdin.write_all(&message)?;
        stdin.flush()?;

        // Read response: 4-byte length prefix + msgpack payload
        let stdout = self
            .process
            .stdout
            .as_mut()
            .ok_or_else(|| anyhow!("tsgo stdout not available"))?;

        let mut len_buf = [0u8; 4];
        stdout.read_exact(&mut len_buf)?;
        let response_len = u32::from_be_bytes(len_buf) as usize;

        let mut response_buf = vec![0u8; response_len];
        stdout.read_exact(&mut response_buf)?;

        // Decode msgpack response
        let (response, _) = msgpack::decode(&response_buf)
            .map_err(|e| anyhow!("Failed to decode tsgo response: {}", e))?;

        // Response is [seq, result_json_string] or [seq, error_json_string]
        if let serde_json::Value::Array(arr) = &response {
            if arr.len() >= 2 {
                // The result is typically a JSON string that needs parsing
                if let Some(result_str) = arr[1].as_str() {
                    let result: serde_json::Value = serde_json::from_str(result_str)
                        .unwrap_or_else(|_| serde_json::Value::String(result_str.to_string()));
                    return Ok(result);
                }
                return Ok(arr[1].clone());
            }
        }

        Ok(response)
    }

    /// Load a project by its tsconfig.json path
    pub fn load_project(&mut self, tsconfig: &Path) -> Result<()> {
        let params = serde_json::json!({
            "file": tsconfig.to_string_lossy()
        });
        self.send_request("loadProject", &params)?;
        Ok(())
    }

    /// Get type information at specific positions in a file (batch query)
    pub fn get_types_at_positions(
        &mut self,
        file: &str,
        positions: &[u32],
    ) -> Result<Vec<Option<TypeInfo>>> {
        if positions.is_empty() {
            return Ok(Vec::new());
        }

        let params = serde_json::json!({
            "file": file,
            "positions": positions,
        });

        let result = self.send_request("getTypesAtPositions", &params)?;

        // Parse the response into TypeInfo structs
        let mut types = Vec::with_capacity(positions.len());
        if let serde_json::Value::Array(arr) = result {
            for item in arr {
                if item.is_null() {
                    types.push(None);
                } else {
                    let flags = item.get("flags").and_then(|f| f.as_u64()).unwrap_or(0) as u32;
                    let type_string = item
                        .get("typeString")
                        .or_else(|| item.get("type"))
                        .and_then(|s| s.as_str())
                        .unwrap_or("any")
                        .to_string();
                    types.push(Some(TypeInfo { flags, type_string }));
                }
            }
        } else {
            // Single type result or unexpected format
            for _ in positions {
                types.push(None);
            }
        }

        Ok(types)
    }

    /// Get type at a single position
    pub fn get_type_at_position(&mut self, file: &str, position: u32) -> Result<Option<TypeInfo>> {
        let params = serde_json::json!({
            "file": file,
            "position": position,
        });

        let result = self.send_request("getTypeAtPosition", &params)?;

        if result.is_null() {
            return Ok(None);
        }

        let flags = result.get("flags").and_then(|f| f.as_u64()).unwrap_or(0) as u32;
        let type_string = result
            .get("typeString")
            .or_else(|| result.get("type"))
            .and_then(|s| s.as_str())
            .unwrap_or("any")
            .to_string();

        Ok(Some(TypeInfo { flags, type_string }))
    }

    /// Shut down the tsgo process gracefully
    pub fn shutdown(&mut self) {
        // Close stdin to signal the process to exit
        drop(self.process.stdin.take());
        // Wait briefly for clean exit, then kill if needed
        let _ = self.process.wait();
    }
}

impl Drop for TsGoClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Convert TypeScript TypeFlags to Perry's Type enum
pub fn ts_flags_to_perry_type(flags: u32, type_string: &str) -> Type {
    // TypeScript TypeFlags constants
    const STRING: u32 = 4;
    const NUMBER: u32 = 8;
    const BOOLEAN: u32 = 16;
    const BIGINT: u32 = 64;
    const STRING_LITERAL: u32 = 128;
    const NUMBER_LITERAL: u32 = 256;
    const BOOLEAN_LITERAL: u32 = 512;
    const VOID: u32 = 16384;
    const UNDEFINED: u32 = 32768;
    const NULL: u32 = 65536;
    const NEVER: u32 = 131072;
    const OBJECT: u32 = 524288;
    const UNION: u32 = 1048576;

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
    if flags & BIGINT != 0 {
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

    let byte_positions: Vec<u32> = positions.iter().map(|(pos, _)| *pos).collect();

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
