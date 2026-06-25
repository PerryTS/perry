//! Child-process stubs, fetch variants, net stubs, crypto, URLSearchParams.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_net_fetch_crypto(
        &mut self,
        func: &mut Function,
        expr: &Expr,
    ) -> bool {
        match expr {
            Expr::ChildProcessExecSync { .. }
            | Expr::ChildProcessSpawnSync { .. }
            | Expr::ChildProcessSpawn { .. }
            | Expr::ChildProcessFork { .. }
            | Expr::ChildProcessExec { .. }
            | Expr::ChildProcessSpawnBackground { .. }
            | Expr::ChildProcessGetProcessStatus(_)
            | Expr::ChildProcessKillProcess(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Fetch ---
            Expr::FetchWithOptions {
                url,
                method,
                body,
                headers,
                headers_dynamic,
                // The WASM fetch backend has no AbortSignal cancellation (it is a
                // separate target from the native binary), so the signal is unused.
                signal: _,
            } => {
                self.emit_expr(func, url);
                self.emit_expr(func, method);
                self.emit_expr(func, body);
                // Build headers object
                if let Some(hexpr) = headers_dynamic {
                    // Dynamically-built headers: leave the object value on the
                    // stack so the runtime enumerates its own properties (#4932).
                    self.emit_expr(func, hexpr);
                } else if headers.is_empty() {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                } else {
                    self.emit_frame_begin(func, 0);
                    self.emit_memcall(func, "object_new", 0);
                    for (key, val) in headers {
                        let key_id = self
                            .emitter
                            .string_map
                            .get(key.as_str())
                            .copied()
                            .unwrap_or(0);
                        let key_bits = (STRING_TAG << 48) | (key_id as u64);
                        self.emit_frame_begin(func, 3);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        self.emit_store_const(func, 1, f64::from_bits(key_bits));
                        self.emit_store_arg(func, 2, val);
                        self.emit_memcall(func, "object_set", 3);
                    }
                }
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "fetch_with_options", 4);
            }
            Expr::FetchGetWithAuth { url, auth_header } => {
                self.emit_expr(func, url);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64)); // method (default GET)
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64)); // body
                                                                                // Build headers object with Authorization
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "object_new", 0);
                let auth_key_id = self
                    .emitter
                    .string_map
                    .get("Authorization")
                    .copied()
                    .unwrap_or(0);
                let auth_key_bits = (STRING_TAG << 48) | (auth_key_id as u64);
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_const(func, 1, f64::from_bits(auth_key_bits));
                self.emit_store_arg(func, 2, auth_header);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "object_set", 3);
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "fetch_with_options", 4);
            }
            Expr::FetchPostWithAuth {
                url,
                auth_header,
                body,
            } => {
                self.emit_expr(func, url);
                // POST method string
                let post_id = self.emitter.string_map.get("POST").copied().unwrap_or(0);
                let post_bits = (STRING_TAG << 48) | (post_id as u64);
                func.instruction(&Instruction::I64Const(post_bits as i64));
                self.emit_expr(func, body);
                // Build headers object with Authorization
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "object_new", 0);
                let auth_key_id = self
                    .emitter
                    .string_map
                    .get("Authorization")
                    .copied()
                    .unwrap_or(0);
                let auth_key_bits = (STRING_TAG << 48) | (auth_key_id as u64);
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_store_const(func, 1, f64::from_bits(auth_key_bits));
                self.emit_store_arg(func, 2, auth_header);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "object_set", 3);
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                self.emit_memcall(func, "fetch_with_options", 4);
            }
            // --- Net stubs ---
            Expr::NetCreateServer { .. }
            | Expr::NetCreateConnection { .. }
            | Expr::NetConnect { .. } => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Crypto ---
            Expr::CryptoRandomUUID => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "crypto_random_uuid", 0);
            }
            Expr::CryptoRandomUUIDv7 => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "crypto_random_uuidv7", 0);
            }
            Expr::CryptoRandomBytes(n) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, n);
                self.emit_memcall(func, "crypto_random_bytes", 1);
            }
            Expr::CryptoSha256(data) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, data);
                self.emit_memcall(func, "crypto_sha256", 1);
            }
            Expr::CryptoMd5(data) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, data);
                self.emit_memcall(func, "crypto_md5", 1);
            }
            // --- URL SearchParams ---
            Expr::UrlSearchParamsNew(init) => {
                if let Some(init_expr) = init {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, init_expr);
                    self.emit_memcall(func, "url_parse", 1);
                    self.emit_frame_begin(func, 1);
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_slot_addr(func, 0);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 3,
                        memory_index: 0,
                    }));
                    self.emit_memcall(func, "url_get_search_params", 1);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::UrlSearchParamsGet { params, name } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_memcall(func, "searchparams_get", 2);
            }
            Expr::UrlSearchParamsHas {
                params,
                name,
                value: _,
            } => {
                // WASM backend doesn't yet model the 2-arg `has(name, value)`
                // variant — drops the optional value and falls back to the
                // 1-arg shape. Native LLVM backend is conformant.
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_memcall_i32(func, "searchparams_has", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(
                    ValType::I64,
                )));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::UrlSearchParamsSet {
                params,
                name,
                value,
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "searchparams_set", 3);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::UrlSearchParamsAppend {
                params,
                name,
                value,
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "searchparams_append", 3);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::UrlSearchParamsDelete {
                params,
                name,
                value: _,
            } => {
                // WASM backend doesn't yet model `delete(name, value)` —
                // value arg ignored, falls back to the 1-arg behavior.
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_memcall_void(func, "searchparams_delete", 2);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::UrlSearchParamsToString(params) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, params);
                self.emit_memcall(func, "searchparams_to_string", 1);
            }
            Expr::UrlSearchParamsGetAll { .. } | Expr::UrlSearchParamsEntries(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            _ => return false,
        }
        true
    }
}
