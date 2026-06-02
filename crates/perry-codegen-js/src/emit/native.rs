use super::*;
use std::fmt::Write as FmtWrite;

impl JsEmitter {
    // --- Native method call mapping ---

    pub(super) fn emit_native_method_call(
        &mut self,
        module: &str,
        class_name: Option<&str>,
        object: Option<&Expr>,
        method: &str,
        args: &[Expr],
    ) {
        let normalized_module = module.strip_prefix("node:").unwrap_or(module);

        match normalized_module {
            "perry/ui" => {
                self.emit_ui_method_call(class_name, object, method, args);
            }
            "perry/system" => {
                self.emit_system_method_call(method, args);
            }
            "perry/audio" => {
                self.emit_audio_method_call(method, args);
            }
            "console" => {
                self.emit_console_call(method, args);
            }
            // --- Timer functions ---
            _ if method == "setTimeout" => {
                self.output.push_str("setTimeout(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            _ if method == "setInterval" => {
                self.output.push_str("setInterval(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            _ if method == "clearTimeout" => {
                self.output.push_str("clearTimeout(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            _ if method == "clearInterval" => {
                self.output.push_str("clearInterval(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            // --- File System (fs module — serve from web file cache) ---
            "fs" => {
                match method {
                    "readFileSync" => {
                        self.output.push_str("__perry.fs_readFileSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "readdirSync" => {
                        self.output.push_str("__perry.fs_readdirSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "isDirectory" => {
                        self.output.push_str("__perry.fs_isDirectory(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "existsSync" => {
                        self.output.push_str("__perry.fs_existsSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "writeFileSync" => {
                        self.output.push_str("__perry.fs_writeFileSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "mkdirSync" => {
                        self.output.push_str("__perry.fs_mkdirSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "unlinkSync" => {
                        self.output.push_str("__perry.fs_unlinkSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    "appendFileSync" => {
                        self.output.push_str("__perry.fs_appendFileSync(");
                        for (i, arg) in args.iter().enumerate() {
                            if i > 0 {
                                self.output.push_str(", ");
                            }
                            self.emit_expr(arg);
                        }
                        self.output.push(')');
                    }
                    _ => {
                        // Graceful fallback — log warning instead of throwing
                        let _ = write!(
                            self.output,
                            "(console.warn('fs.{} not available in browser'), \"\")",
                            method
                        );
                    }
                }
            }
            // --- child_process (stub in browser) ---
            "child_process" => match method {
                "execSync" => {
                    self.output.push_str("__perry.child_process_execSync(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
                _ => {
                    let _ = write!(
                        self.output,
                        "(console.warn('child_process.{} not available in browser'), \"\")",
                        method
                    );
                }
            },
            // --- node-fetch (Perry native SSE streaming → Fetch API on web) ---
            "node-fetch" => match method {
                "streamStart" => {
                    self.output.push_str("__perry.stream_start(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
                "streamPoll" => {
                    self.output.push_str("__perry.stream_poll(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
                "streamStatus" => {
                    self.output.push_str("__perry.stream_status(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
                "streamClose" => {
                    self.output.push_str("__perry.stream_close(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
                _ => {
                    let _ = write!(
                        self.output,
                        "(console.warn('node-fetch.{} not available in browser'), \"\")",
                        method
                    );
                }
            },
            // --- child_process: spawnBackground (stub) ---
            _ if method == "spawnBackground" => {
                self.output
                    .push_str("(console.warn('spawnBackground not available in browser'), 0)");
            }
            // --- Fastify/HTTP (throw in browser) ---
            "fastify" | "ws" | "mysql2" | "mysql2/promise" | "pg" | "net" | "worker_threads" => {
                let _ = write!(
                    self.output,
                    "((() => {{ throw new Error('{} not available in browser'); }})())",
                    normalized_module
                );
            }
            // --- Events module ---
            "events"
                if method == "on"
                    || method == "addEventListener"
                    || method == "emit"
                    || method == "removeListener" =>
            {
                if let Some(obj) = object {
                    self.emit_expr(obj);
                    let _ = write!(self.output, ".{}(", method);
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                } else {
                    self.output.push_str("undefined");
                }
            }
            // --- Default: try to emit as method call on object ---
            _ => {
                if let Some(obj) = object {
                    self.emit_expr(obj);
                    let _ = write!(self.output, ".{}(", method);
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                } else {
                    // Static-style call - just emit as function call
                    let _ = write!(self.output, "{}(", method);
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                }
            }
        }
    }
}
