use super::*;
use std::fmt::Write as FmtWrite;

impl JsEmitter {
    // --- Expression emission ---

    pub fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            // --- Literals ---
            Expr::Undefined => self.output.push_str("undefined"),
            Expr::Null => self.output.push_str("null"),
            Expr::Bool(b) => self.output.push_str(if *b { "true" } else { "false" }),
            Expr::Number(n) => {
                if n.is_nan() {
                    self.output.push_str("NaN");
                } else if n.is_infinite() {
                    if *n > 0.0 {
                        self.output.push_str("Infinity");
                    } else {
                        self.output.push_str("-Infinity");
                    }
                } else if *n == 0.0 && n.is_sign_negative() {
                    self.output.push_str("-0");
                } else {
                    // Use integer format when possible for cleaner output
                    let i = *n as i64;
                    if i as f64 == *n && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                        let _ = write!(self.output, "{}", i);
                    } else {
                        let _ = write!(self.output, "{}", n);
                    }
                }
            }
            Expr::Integer(i) => {
                let _ = write!(self.output, "{}", i);
            }
            Expr::BigInt(s) => {
                let _ = write!(self.output, "{}n", s);
            }
            Expr::String(s) => {
                self.output.push_str(&self.quote_string(s));
            }
            Expr::I18nString { key, .. } => {
                // JS codegen: emit as regular string (i18n handled by JS runtime)
                self.output.push_str(&self.quote_string(key));
            }

            // --- Variables ---
            Expr::LocalGet(id) => {
                let name = self.get_local_name(*id);
                self.output.push_str(&name);
            }
            Expr::LocalSet(id, val) => {
                let name = self.get_local_name(*id);
                let _ = write!(self.output, "({} = ", name);
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::GlobalGet(id) => {
                let name = self.get_global_name(*id);
                // GlobalGet(0) for unregistered globals is the implicit console object
                if name.starts_with("_g") && !self.global_names.contains_key(id) {
                    self.output.push_str("console");
                } else {
                    self.output.push_str(&name);
                }
            }
            Expr::GlobalSet(id, val) => {
                let name = self.get_global_name(*id);
                let _ = write!(self.output, "({} = ", name);
                self.emit_expr(val);
                self.output.push(')');
            }

            // --- Update ---
            Expr::Update { id, op, prefix } => {
                let name = self.get_local_name(*id);
                let op_str = match op {
                    UpdateOp::Increment => "++",
                    UpdateOp::Decrement => "--",
                };
                if *prefix {
                    let _ = write!(self.output, "{}{}", op_str, name);
                } else {
                    let _ = write!(self.output, "{}{}", name, op_str);
                }
            }

            // --- Binary operations ---
            Expr::Binary { op, left, right } => {
                self.output.push('(');
                self.emit_expr(left);
                let op_str = match op {
                    BinaryOp::Add => " + ",
                    BinaryOp::Sub => " - ",
                    BinaryOp::Mul => " * ",
                    BinaryOp::Div => " / ",
                    BinaryOp::Mod => " % ",
                    BinaryOp::Pow => " ** ",
                    BinaryOp::BitAnd => " & ",
                    BinaryOp::BitOr => " | ",
                    BinaryOp::BitXor => " ^ ",
                    BinaryOp::Shl => " << ",
                    BinaryOp::Shr => " >> ",
                    BinaryOp::UShr => " >>> ",
                };
                self.output.push_str(op_str);
                self.emit_expr(right);
                self.output.push(')');
            }

            // --- Unary operations ---
            Expr::Unary { op, operand } => {
                match op {
                    UnaryOp::Neg => { self.output.push_str("(-"); self.emit_expr(operand); self.output.push(')'); }
                    UnaryOp::Not => { self.output.push_str("(!"); self.emit_expr(operand); self.output.push(')'); }
                    UnaryOp::BitNot => { self.output.push_str("(~"); self.emit_expr(operand); self.output.push(')'); }
                    UnaryOp::Pos => { self.output.push_str("(+"); self.emit_expr(operand); self.output.push(')'); }
                }
            }

            // --- Comparison ---
            Expr::Compare { op, left, right } => {
                self.output.push('(');
                self.emit_expr(left);
                let op_str = match op {
                    CompareOp::Eq => " === ",
                    CompareOp::Ne => " !== ",
                    CompareOp::LooseEq => " == ",
                    CompareOp::LooseNe => " != ",
                    CompareOp::Lt => " < ",
                    CompareOp::Le => " <= ",
                    CompareOp::Gt => " > ",
                    CompareOp::Ge => " >= ",
                };
                self.output.push_str(op_str);
                self.emit_expr(right);
                self.output.push(')');
            }

            // --- Logical ---
            Expr::Logical { op, left, right } => {
                self.output.push('(');
                self.emit_expr(left);
                let op_str = match op {
                    LogicalOp::And => " && ",
                    LogicalOp::Or => " || ",
                    LogicalOp::Coalesce => " ?? ",
                };
                self.output.push_str(op_str);
                self.emit_expr(right);
                self.output.push(')');
            }

            // --- Function calls ---
            Expr::Call { callee, args, .. } => {
                self.emit_expr(callee);
                self.output.push('(');
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }

            Expr::CallSpread { callee, args, .. } => {
                self.emit_expr(callee);
                self.output.push('(');
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    match arg {
                        CallArg::Expr(e) => self.emit_expr(e),
                        CallArg::Spread(e) => {
                            self.output.push_str("...");
                            self.emit_expr(e);
                        }
                    }
                }
                self.output.push(')');
            }

            // --- Function reference ---
            Expr::FuncRef(id) => {
                let name = self.get_func_name(*id);
                self.output.push_str(&name);
            }

            Expr::ExternFuncRef { name, .. } => {
                self.output.push_str(name);
            }

            // --- Native module handling ---
            Expr::NativeModuleRef(_module) => {
                // Native module reference - in web, this is a no-op or maps to a shim
                self.output.push_str("undefined");
            }

            Expr::NativeMethodCall { module, class_name, object, method, args } => {
                self.emit_native_method_call(module, class_name.as_deref(), object.as_deref(), method, args);
            }

            // --- Property access ---
            Expr::PropertyGet { object, property, .. } => {
                self.emit_expr(object);
                if is_valid_identifier(property) {
                    let _ = write!(self.output, ".{}", property);
                } else {
                    let _ = write!(self.output, "[{}]", self.quote_string(property));
                }
            }
            Expr::PropertySet { object, property, value } => {
                self.output.push('(');
                self.emit_expr(object);
                if is_valid_identifier(property) {
                    let _ = write!(self.output, ".{}", property);
                } else {
                    let _ = write!(self.output, "[{}]", self.quote_string(property));
                }
                self.output.push_str(" = ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::PropertyUpdate { object, property, op, prefix } => {
                let op_str = match op {
                    BinaryOp::Add => "++",
                    BinaryOp::Sub => "--",
                    _ => "++",
                };
                if *prefix {
                    let _ = write!(self.output, "{}", op_str);
                    self.emit_expr(object);
                    let _ = write!(self.output, ".{}", property);
                } else {
                    self.emit_expr(object);
                    let _ = write!(self.output, ".{}{}", property, op_str);
                }
            }

            // --- Index access ---
            Expr::IndexGet { object, index } => {
                self.emit_expr(object);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push(']');
            }
            Expr::IndexSet { object, index, value } => {
                self.output.push('(');
                self.emit_expr(object);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push_str("] = ");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::IndexUpdate { object, index, op, prefix } => {
                let op_str = match op {
                    BinaryOp::Add => "++",
                    BinaryOp::Sub => "--",
                    _ => "++",
                };
                if *prefix {
                    self.output.push_str(op_str);
                    self.emit_expr(object);
                    self.output.push('[');
                    self.emit_expr(index);
                    self.output.push(']');
                } else {
                    self.emit_expr(object);
                    self.output.push('[');
                    self.emit_expr(index);
                    self.output.push(']');
                    self.output.push_str(op_str);
                }
            }

            // --- Object literal ---
            Expr::Object(fields) => {
                self.output.push('{');
                for (i, (key, val)) in fields.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    if is_valid_identifier(key) {
                        self.output.push_str(key);
                    } else {
                        self.output.push_str(&self.quote_string(key));
                    }
                    self.output.push_str(": ");
                    self.emit_expr(val);
                }
                self.output.push('}');
            }
            Expr::ObjectSpread { parts } => {
                self.output.push('{');
                let mut first = true;
                for (key_opt, val) in parts.iter() {
                    if !first { self.output.push_str(", "); }
                    first = false;
                    match key_opt {
                        None => {
                            self.output.push_str("...(");
                            self.emit_expr(val);
                            self.output.push(')');
                        }
                        Some(key) => {
                            if is_valid_identifier(key) {
                                self.output.push_str(key);
                            } else {
                                self.output.push_str(&self.quote_string(key));
                            }
                            self.output.push_str(": ");
                            self.emit_expr(val);
                        }
                    }
                }
                self.output.push('}');
            }

            // --- Array literal ---
            Expr::Array(elements) => {
                self.output.push('[');
                for (i, el) in elements.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(el);
                }
                self.output.push(']');
            }

            Expr::ArraySpread(elements) => {
                self.output.push('[');
                for (i, el) in elements.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    match el {
                        ArrayElement::Expr(e) => self.emit_expr(e),
                        ArrayElement::Spread(e) => {
                            self.output.push_str("...");
                            self.emit_expr(e);
                        }
                        ArrayElement::Hole => {}
                    }
                }
                self.output.push(']');
            }

            // --- Conditional (ternary) ---
            Expr::Conditional { condition, then_expr, else_expr } => {
                self.output.push('(');
                self.emit_expr(condition);
                self.output.push_str(" ? ");
                self.emit_expr(then_expr);
                self.output.push_str(" : ");
                self.emit_expr(else_expr);
                self.output.push(')');
            }

            // --- Type operations ---
            Expr::TypeOf(operand) => {
                self.output.push_str("typeof ");
                self.emit_expr(operand);
            }
            Expr::Void(operand) => {
                self.output.push_str("void ");
                self.emit_expr(operand);
            }
            Expr::InstanceOf { expr, ty, .. } => {
                self.output.push('(');
                self.emit_expr(expr);
                let _ = write!(self.output, " instanceof {})", ty);
            }
            Expr::In { property, object } => {
                self.output.push('(');
                self.emit_expr(property);
                self.output.push_str(" in ");
                self.emit_expr(object);
                self.output.push(')');
            }

            // --- Await / Yield ---
            Expr::Await(expr) => {
                self.output.push_str("(await ");
                self.emit_expr(expr);
                self.output.push(')');
            }
            Expr::Yield { value, delegate } => {
                if *delegate {
                    self.output.push_str("yield* ");
                } else {
                    self.output.push_str("yield ");
                }
                if let Some(val) = value {
                    self.emit_expr(val);
                }
            }

            // --- New expression ---
            Expr::New { class_name, args, .. } => {
                let _ = write!(self.output, "new {}(", class_name);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::NewDynamic { callee, args, .. } => {
                self.output.push_str("new (");
                self.emit_expr(callee);
                self.output.push_str(")(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }

            // --- Class/Enum reference ---
            Expr::ClassRef(name) => {
                self.output.push_str(name);
            }
            Expr::EnumMember { enum_name, member_name } => {
                let _ = write!(self.output, "{}.{}", enum_name, member_name);
            }

            // --- Static field/method ---
            Expr::StaticFieldGet { class_name, field_name } => {
                let _ = write!(self.output, "{}.{}", class_name, field_name);
            }
            Expr::StaticFieldSet { class_name, field_name, value } => {
                let _ = write!(self.output, "({}.{} = ", class_name, field_name);
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::StaticMethodCall { class_name, method_name, args } => {
                let _ = write!(self.output, "{}.{}(", class_name, method_name);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }

            // --- This / Super ---
            Expr::This => self.output.push_str("this"),
            Expr::SuperCall(args) => {
                self.output.push_str("super(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }
            Expr::SuperMethodCall { method, args } => {
                let _ = write!(self.output, "super.{}(", method);
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.output.push_str(", "); }
                    self.emit_expr(arg);
                }
                self.output.push(')');
            }

            // --- Process/Environment ---
            Expr::EnvGet(name) => {
                // In browser, no real env vars
                let _ = write!(self.output, "(typeof process !== 'undefined' ? process.env.{} : undefined)", name);
            }
            Expr::EnvGetDynamic(expr) => {
                self.output.push_str("(typeof process !== 'undefined' ? process.env[");
                self.emit_expr(expr);
                self.output.push_str("] : undefined)");
            }
            Expr::ProcessUptime => {
                self.output.push_str("(performance.now() / 1000)");
            }
            Expr::ProcessCwd => {
                self.output.push_str("(typeof process !== 'undefined' ? process.cwd() : '/')");
            }
            Expr::ProcessArgv => {
                self.output.push_str("(typeof process !== 'undefined' ? process.argv : [])");
            }
            Expr::ProcessMemoryUsage => {
                self.output.push_str("(typeof process !== 'undefined' ? process.memoryUsage() : {rss: 0, heapTotal: 0, heapUsed: 0, external: 0, arrayBuffers: 0})");
            }
            Expr::ProcessThreadCpuUsage(prior) => {
                self.output.push_str("(typeof process !== 'undefined' && typeof process.threadCpuUsage === 'function' ? process.threadCpuUsage(");
                if let Some(p) = prior {
                    self.emit_expr(p);
                }
                self.output.push_str(") : {user: 0, system: 0})");
            }
            Expr::ProcessAvailableMemory => {
                self.output.push_str("(typeof process !== 'undefined' && typeof process.availableMemory === 'function' ? process.availableMemory() : 0)");
            }
            Expr::ProcessConstrainedMemory => {
                self.output.push_str("(typeof process !== 'undefined' && typeof process.constrainedMemory === 'function' ? process.constrainedMemory() : 0)");
            }
            Expr::ProcessPosixCredential(kind) => {
                let method = match kind {
                    perry_hir::PosixCredentialKind::Uid => "getuid",
                    perry_hir::PosixCredentialKind::Euid => "geteuid",
                    perry_hir::PosixCredentialKind::Gid => "getgid",
                    perry_hir::PosixCredentialKind::Egid => "getegid",
                };
                let _ = write!(
                    self.output,
                    "(typeof process !== 'undefined' && typeof process.{m} === 'function' ? process.{m}() : 0)",
                    m = method
                );
            }
            Expr::ProcessEmitWarning(args) => {
                self.output.push_str("(typeof process !== 'undefined' && typeof process.emitWarning === 'function' ? process.emitWarning(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(a);
                }
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessCpuUsage(prior) => {
                self.output.push_str("(typeof process !== 'undefined' && typeof process.cpuUsage === 'function' ? process.cpuUsage(");
                if let Some(p) = prior {
                    self.emit_expr(p);
                }
                self.output.push_str(") : {user: 0, system: 0})");
            }
            Expr::ProcessResourceUsage => {
                self.output.push_str("(typeof process !== 'undefined' && typeof process.resourceUsage === 'function' ? process.resourceUsage() : {})");
            }
            Expr::ProcessActiveResourcesInfo => {
                self.output.push_str("(typeof process !== 'undefined' && typeof process.getActiveResourcesInfo === 'function' ? process.getActiveResourcesInfo() : [])");
            }
            Expr::ProcessPid => {
                self.output.push_str("(typeof process !== 'undefined' ? process.pid : 0)");
            }
            Expr::ProcessPpid => {
                self.output.push_str("(typeof process !== 'undefined' ? process.ppid : 0)");
            }
            Expr::ProcessVersion => {
                self.output.push_str("(typeof process !== 'undefined' ? process.version : 'v22.0.0')");
            }
            Expr::ProcessVersions => {
                self.output.push_str("(typeof process !== 'undefined' ? process.versions : {node:'22.0.0', v8:'12.4.254.21'})");
            }
            Expr::ProcessHrtimeBigint => {
                self.output.push_str("(typeof process !== 'undefined' ? process.hrtime.bigint() : BigInt(Date.now()) * 1000000n)");
            }
            Expr::ProcessHrtime(prior) => {
                self.output.push_str("(typeof process !== 'undefined' && typeof process.hrtime === 'function' ? process.hrtime(");
                if let Some(p) = prior {
                    self.emit_expr(p);
                }
                self.output.push_str(") : [0, 0])");
            }
            Expr::ProcessTitle => {
                self.output.push_str("(typeof process !== 'undefined' ? process.title : '')");
            }
            Expr::ProcessSetTitle(value) => {
                self.output.push_str("(typeof process !== 'undefined' ? (process.title = ");
                self.emit_expr(value);
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessNextTick { callback, args } => {
                self.output.push_str("(typeof process !== 'undefined' ? process.nextTick(");
                self.emit_expr(callback);
                for a in args {
                    self.output.push_str(", ");
                    self.emit_expr(a);
                }
                self.output.push_str(") : queueMicrotask(");
                self.emit_expr(callback);
                self.output.push_str("))");
            }
            Expr::ProcessOn { event, handler } => {
                self.output.push_str("(typeof process !== 'undefined' ? process.on(");
                self.emit_expr(event);
                self.output.push_str(", ");
                self.emit_expr(handler);
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessStdinSetRawMode(enabled) => {
                self.output.push_str("(typeof process !== 'undefined' && process.stdin && typeof process.stdin.setRawMode === 'function' ? process.stdin.setRawMode(");
                self.emit_expr(enabled);
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessStdinOn { event, handler } => {
                self.output.push_str("(typeof process !== 'undefined' && process.stdin ? process.stdin.on(");
                self.emit_expr(event);
                self.output.push_str(", ");
                self.emit_expr(handler);
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessStdinRemoveListener { event, handler } => {
                self.output.push_str("(typeof process !== 'undefined' && process.stdin ? (process.stdin.removeListener || process.stdin.off).call(process.stdin, ");
                self.emit_expr(event);
                self.output.push_str(", ");
                self.emit_expr(handler);
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessStdinLifecycle(method) => {
                let name = match method {
                    perry_hir::ProcessStdinLifecycleMethod::Pause => "pause",
                    perry_hir::ProcessStdinLifecycleMethod::Resume => "resume",
                    perry_hir::ProcessStdinLifecycleMethod::Unref => "unref",
                    perry_hir::ProcessStdinLifecycleMethod::Ref => "ref",
                    perry_hir::ProcessStdinLifecycleMethod::Destroy => "destroy",
                };
                self.output.push_str("(typeof process !== 'undefined' && process.stdin && typeof process.stdin.");
                self.output.push_str(name);
                self.output.push_str(" === 'function' ? process.stdin.");
                self.output.push_str(name);
                self.output.push_str("() : undefined)");
            }
            Expr::ProcessChdir(dir) => {
                self.output.push_str("(typeof process !== 'undefined' ? process.chdir(");
                self.emit_expr(dir);
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessKill { pid, signal } => {
                self.output.push_str("(typeof process !== 'undefined' ? process.kill(");
                self.emit_expr(pid);
                if let Some(s) = signal {
                    self.output.push_str(", ");
                    self.emit_expr(s);
                }
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessExit(code) => {
                self.output.push_str("(typeof process !== 'undefined' ? process.exit(");
                if let Some(c) = code {
                    self.emit_expr(c);
                } else {
                    self.output.push('0');
                }
                self.output.push_str(") : undefined)");
            }
            Expr::ProcessAbort => {
                self.output.push_str("(typeof process !== 'undefined' ? process.abort() : undefined)");
            }
            Expr::ProcessUmask(mask) => {
                self.output.push_str("(typeof process !== 'undefined' ? process.umask(");
                if let Some(m) = mask {
                    self.emit_expr(m);
                }
                self.output.push_str(") : 0)");
            }
            Expr::ProcessStdin => {
                self.output.push_str("(typeof process !== 'undefined' ? process.stdin : { write: () => true })");
            }
            Expr::ProcessStdout => {
                self.output.push_str("(typeof process !== 'undefined' ? process.stdout : { write: (s) => { console.log(s); return true; } })");
            }
            Expr::ProcessStderr => {
                self.output.push_str("(typeof process !== 'undefined' ? process.stderr : { write: (s) => { console.error(s); return true; } })");
            }

            // --- File System (web-compatible stubs) ---
            Expr::FsReadFileSync(path) => {
                self.output.push_str("__perry.fs_readFileSync(");
                self.emit_expr(path);
                self.output.push(')');
            }
            Expr::FsWriteFileSync(_, _) |
            Expr::FsMkdirSync(_) |
            Expr::FsUnlinkSync(_) |
            Expr::FsAppendFileSync(_, _) |
            Expr::FsReadFileBinary(_) |
            Expr::FsRmRecursive(_) => {
                self.output.push_str("((() => { throw new Error('fs write operations not available in browser'); })())");
            }
            Expr::FsExistsSync(path) => {
                self.output.push_str("__perry.fs_existsSync(");
                self.emit_expr(path);
                self.output.push(')');
            }

            // --- Path operations ---
            Expr::PathJoin(a, b) => {
                self.output.push_str("__perry.path.join(");
                self.emit_expr(a);
                self.output.push_str(", ");
                self.emit_expr(b);
                self.output.push(')');
            }
            Expr::PathWin32Join(a, b) => {
                self.output.push_str("__perry.path.win32.join(");
                self.emit_expr(a);
                self.output.push_str(", ");
                self.emit_expr(b);
                self.output.push(')');
            }
            Expr::PathWin32 { method, args } => {
                use perry_hir::PathWin32Method;
                let name = match method {
                    PathWin32Method::Dirname => "dirname",
                    PathWin32Method::Basename | PathWin32Method::BasenameExt => "basename",
                    PathWin32Method::Extname => "extname",
                    PathWin32Method::IsAbsolute => "isAbsolute",
                    PathWin32Method::Normalize => "normalize",
                    PathWin32Method::Parse => "parse",
                    PathWin32Method::Format => "format",
                    PathWin32Method::Relative => "relative",
                    PathWin32Method::Resolve | PathWin32Method::ResolveJoin => "resolve",
                    PathWin32Method::ToNamespacedPath => "toNamespacedPath",
                    PathWin32Method::MatchesGlob => "matchesGlob",
                };
                self.output.push_str("__perry.path.win32.");
                self.output.push_str(name);
                self.output.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            Expr::PathDirname(p) => {
                self.output.push_str("__perry.path.dirname(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathBasename(p) => {
                self.output.push_str("__perry.path.basename(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathExtname(p) => {
                self.output.push_str("__perry.path.extname(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathResolve(p) => {
                self.output.push_str("__perry.path.resolve(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathIsAbsolute(p) => {
                self.output.push_str("__perry.path.isAbsolute(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathRelative(from, to) => {
                self.output.push_str("__perry.path.relative(");
                self.emit_expr(from);
                self.output.push_str(", ");
                self.emit_expr(to);
                self.output.push(')');
            }
            Expr::PathNormalize(p) => {
                self.output.push_str("__perry.path.normalize(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathParse(p) => {
                self.output.push_str("__perry.path.parse(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathFormat(p) => {
                self.output.push_str("__perry.path.format(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathBasenameExt(p, ext) => {
                self.output.push_str("__perry.path.basename(");
                self.emit_expr(p);
                self.output.push_str(", ");
                self.emit_expr(ext);
                self.output.push(')');
            }
            Expr::PathSep => {
                self.output.push_str("__perry.path.sep");
            }
            Expr::PathDelimiter => {
                self.output.push_str("__perry.path.delimiter");
            }
            Expr::PathToNamespacedPath(p) => {
                self.output.push_str("__perry.path.toNamespacedPath(");
                self.emit_expr(p);
                self.output.push(')');
            }
            Expr::PathMatchesGlob(p, pat) => {
                self.output.push_str("__perry.path.matchesGlob(");
                self.emit_expr(p);
                self.output.push_str(", ");
                self.emit_expr(pat);
                self.output.push(')');
            }
            Expr::PathResolveJoin(a, b) => {
                // Match Node's path.resolve(a, b) two-arg behavior.
                self.output.push_str("__perry.path.resolve(");
                self.emit_expr(a);
                self.output.push_str(", ");
                self.emit_expr(b);
                self.output.push(')');
            }

            // --- WeakRef and FinalizationRegistry ---
            Expr::WeakRefNew(target) => {
                self.output.push_str("new WeakRef(");
                self.emit_expr(target);
                self.output.push(')');
            }
            Expr::WeakRefDeref(weakref_expr) => {
                self.output.push('(');
                self.emit_expr(weakref_expr);
                self.output.push_str(").deref()");
            }
            Expr::FinalizationRegistryNew(callback) => {
                self.output.push_str("new FinalizationRegistry(");
                self.emit_expr(callback);
                self.output.push(')');
            }
            Expr::FinalizationRegistryRegister { registry, target, held, token } => {
                self.output.push('(');
                self.emit_expr(registry);
                self.output.push_str(").register(");
                self.emit_expr(target);
                self.output.push_str(", ");
                self.emit_expr(held);
                if let Some(t) = token {
                    self.output.push_str(", ");
                    self.emit_expr(t);
                }
                self.output.push(')');
            }
            Expr::FinalizationRegistryUnregister { registry, token } => {
                self.output.push('(');
                self.emit_expr(registry);
                self.output.push_str(").unregister(");
                self.emit_expr(token);
                self.output.push(')');
            }

            // --- URL ---
            Expr::FileURLToPath(u) => {
                self.output.push_str("(new URL(");
                self.emit_expr(u);
                self.output.push_str(")).pathname");
            }

            // --- JSON ---
            Expr::JsonParse(val) => {
                self.output.push_str("JSON.parse(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::JsonStringify(val) => {
                self.output.push_str("JSON.stringify(");
                self.emit_expr(val);
                self.output.push(')');
            }
            Expr::JsonStringifyPretty { value, replacer, space } => {
                self.output.push_str("JSON.stringify(");
                self.emit_expr(value);
                self.output.push_str(", ");
                if let Some(r) = replacer { self.emit_expr(r); } else { self.output.push_str("null"); }
                self.output.push_str(", ");
                self.emit_expr(space);
                self.output.push(')');
            }
            Expr::JsonParseReviver { text, reviver } | Expr::JsonParseWithReviver(text, reviver) => {
                self.output.push_str("JSON.parse(");
                self.emit_expr(text);
                self.output.push_str(", ");
                self.emit_expr(reviver);
                self.output.push(')');
            }
            Expr::JsonStringifyFull(value, replacer, spacer) => {
                self.output.push_str("JSON.stringify(");
                self.emit_expr(value);
                self.output.push_str(", ");
                self.emit_expr(replacer);
                self.output.push_str(", ");
                self.emit_expr(spacer);
                self.output.push(')');
            }

            // --- Math ---
            Expr::MathFloor(x) => { self.emit_math_unary("Math.floor", x); }
            Expr::MathCeil(x) => { self.emit_math_unary("Math.ceil", x); }
            Expr::MathRound(x) => { self.emit_math_unary("Math.round", x); }
            Expr::MathTrunc(x) => { self.emit_math_unary("Math.trunc", x); }
            Expr::MathSign(x) => { self.emit_math_unary("Math.sign", x); }
            Expr::MathAbs(x) => { self.emit_math_unary("Math.abs", x); }
            Expr::MathSqrt(x) => { self.emit_math_unary("Math.sqrt", x); }
            Expr::MathLog(x) => { self.emit_math_unary("Math.log", x); }
            Expr::MathLog2(x) => { self.emit_math_unary("Math.log2", x); }
            Expr::MathLog10(x) => { self.emit_math_unary("Math.log10", x); }
            Expr::MathSin(x) => { self.emit_math_unary("Math.sin", x); }
            Expr::MathCos(x) => { self.emit_math_unary("Math.cos", x); }
            Expr::MathTan(x) => { self.emit_math_unary("Math.tan", x); }
            Expr::MathAsin(x) => { self.emit_math_unary("Math.asin", x); }
            Expr::MathAcos(x) => { self.emit_math_unary("Math.acos", x); }
            Expr::MathAtan(x) => { self.emit_math_unary("Math.atan", x); }
            Expr::MathAtan2(y, x) => {
                self.output.push_str("Math.atan2(");
                self.emit_expr(y);
                self.output.push_str(", ");
                self.emit_expr(x);
                self.output.push(')');
            }
            Expr::MathCbrt(x) => { self.emit_math_unary("Math.cbrt", x); }
            Expr::MathFround(x) => { self.emit_math_unary("Math.fround", x); }
            Expr::MathClz32(x) => { self.emit_math_unary("Math.clz32", x); }
            Expr::MathExpm1(x) => { self.emit_math_unary("Math.expm1", x); }
            Expr::MathLog1p(x) => { self.emit_math_unary("Math.log1p", x); }
            Expr::MathSinh(x) => { self.emit_math_unary("Math.sinh", x); }
            Expr::MathCosh(x) => { self.emit_math_unary("Math.cosh", x); }
            Expr::MathTanh(x) => { self.emit_math_unary("Math.tanh", x); }
            Expr::MathAsinh(x) => { self.emit_math_unary("Math.asinh", x); }
            Expr::MathAcosh(x) => { self.emit_math_unary("Math.acosh", x); }
            Expr::MathAtanh(x) => { self.emit_math_unary("Math.atanh", x); }
            Expr::MathExp(x) => { self.emit_math_unary("Math.exp", x); }
            Expr::MathHypot(args) => { self.emit_math_variadic("Math.hypot", args); }
            Expr::MathPow(base, exp) => {
                self.output.push_str("Math.pow(");
                self.emit_expr(base);
                self.output.push_str(", ");
                self.emit_expr(exp);
                self.output.push(')');
            }
            Expr::MathMin(args) => { self.emit_math_variadic("Math.min", args); }
            Expr::MathMax(args) => { self.emit_math_variadic("Math.max", args); }
            Expr::MathMinSpread(arr) => {
                self.output.push_str("Math.min(...");
                self.emit_expr(arr);
                self.output.push(')');
            }
            Expr::MathMaxSpread(arr) => {
                self.output.push_str("Math.max(...");
                self.emit_expr(arr);
                self.output.push(')');
            }
            Expr::MathRandom => self.output.push_str("Math.random()"),

            // --- Crypto ---
            Expr::CryptoRandomBytes(size) => {
                self.output.push_str("Array.from(crypto.getRandomValues(new Uint8Array(");
                self.emit_expr(size);
                self.output.push_str("))).map(b => b.toString(16).padStart(2, '0')).join('')");
            }
            Expr::CryptoRandomUUID => {
                self.output.push_str("crypto.randomUUID()");
            }
            Expr::CryptoRandomUUIDv7 => {
                self.output.push_str("crypto.randomUUIDv7()");
            }
            Expr::CryptoSha256(data) => {
                // Use SubtleCrypto (async in browser, but we emit it inline)
                self.output.push_str("(await (async () => { const d = new TextEncoder().encode(");
                self.emit_expr(data);
                self.output.push_str("); const h = await crypto.subtle.digest('SHA-256', d); return Array.from(new Uint8Array(h)).map(b => b.toString(16).padStart(2, '0')).join(''); })())");
            }
            Expr::CryptoMd5(_) => {
                self.output.push_str("((() => { throw new Error('MD5 not available in browser crypto API'); })())");
            }

            // --- OS (browser alternatives) ---
            Expr::OsPlatform => self.output.push_str("'browser'"),
            Expr::OsArch => self.output.push_str("'wasm'"),
            Expr::OsHostname => self.output.push_str("location.hostname"),
            Expr::OsHomedir => self.output.push_str("'/'"),
            Expr::OsTmpdir => self.output.push_str("'/tmp'"),
            Expr::OsTotalmem => self.output.push_str("(navigator.deviceMemory ? navigator.deviceMemory * 1024 * 1024 * 1024 : 4294967296)"),
            Expr::OsFreemem => self.output.push_str("(navigator.deviceMemory ? navigator.deviceMemory * 1024 * 1024 * 1024 : 4294967296)"),
            Expr::OsUptime => self.output.push_str("(performance.now() / 1000)"),
            Expr::OsType => self.output.push_str("'Browser'"),
            Expr::OsRelease => self.output.push_str("navigator.userAgent"),
            Expr::OsCpus => self.output.push_str("(Array.from({length: navigator.hardwareConcurrency || 4}, () => ({model: 'unknown', speed: 0})))"),
            Expr::OsNetworkInterfaces => self.output.push_str("({})"),
            Expr::OsUserInfo => self.output.push_str("({username: 'browser', homedir: '/', shell: ''})"),
            Expr::OsEOL => self.output.push_str("'\\n'"),

            // --- Buffer (basic browser polyfill using Uint8Array) ---
            Expr::BufferFrom { data, encoding } => {
                self.output.push_str("new TextEncoder().encode(");
                self.emit_expr(data);
                self.output.push(')');
                let _ = encoding; // encoding not used in simple polyfill
            }
            Expr::BufferFromArrayBuffer {
                data,
                byte_offset,
                length,
            } => {
                self.output.push_str("new Uint8Array(");
                self.emit_expr(data);
                self.output.push_str(", ");
                self.emit_expr(byte_offset);
                if let Some(len) = length {
                    self.output.push_str(", ");
                    self.emit_expr(len);
                }
                self.output.push(')');
            }
            Expr::BufferAlloc { size, fill, .. } => {
                self.output.push_str("new Uint8Array(");
                self.emit_expr(size);
                self.output.push(')');
                if let Some(f) = fill {
                    self.output.push_str(".fill(");
                    self.emit_expr(f);
                    self.output.push(')');
                }
            }
            Expr::BufferAllocUnsafe(size) => {
                self.output.push_str("new Uint8Array(");
                self.emit_expr(size);
                self.output.push(')');
            }
            Expr::BufferConcat(list) => {
                // Simple concat implementation
                self.output.push_str("((() => { const _arrs = ");
                self.emit_expr(list);
                self.output.push_str("; const _len = _arrs.reduce((a,b) => a + b.length, 0); const _r = new Uint8Array(_len); let _off = 0; for (const _a of _arrs) { _r.set(_a, _off); _off += _a.length; } return _r; })())");
            }
            Expr::BufferIsBuffer(obj) => {
                self.output.push('(');
                self.emit_expr(obj);
                self.output.push_str(" instanceof Uint8Array)");
            }
            Expr::BufferByteLength { data, encoding } => {
                self.output.push_str("new TextEncoder().encode(");
                self.emit_expr(data);
                self.output.push_str(").length");
                let _ = encoding;
            }
            Expr::BufferToString { buffer, .. } => {
                self.output.push_str("new TextDecoder().decode(");
                self.emit_expr(buffer);
                self.output.push(')');
            }
            Expr::BufferLength(buf) => {
                self.emit_expr(buf);
                self.output.push_str(".length");
            }
            Expr::BufferSlice { buffer, start, end } => {
                self.emit_expr(buffer);
                self.output.push_str(".slice(");
                if let Some(s) = start { self.emit_expr(s); } else { self.output.push('0'); }
                if let Some(e) = end {
                    self.output.push_str(", ");
                    self.emit_expr(e);
                }
                self.output.push(')');
            }
            Expr::BufferFill { buffer, value, .. } => {
                self.emit_expr(buffer);
                self.output.push_str(".fill(");
                self.emit_expr(value);
                self.output.push(')');
            }
            Expr::BufferCopy { source, target, target_start, source_start, source_end } => {
                self.emit_expr(target);
                self.output.push_str(".set(");
                self.emit_expr(source);
                if let Some(ss) = source_start {
                    self.output.push_str(".slice(");
                    self.emit_expr(ss);
                    if let Some(se) = source_end {
                        self.output.push_str(", ");
                        self.emit_expr(se);
                    }
                    self.output.push(')');
                }
                if let Some(ts) = target_start {
                    self.output.push_str(", ");
                    self.emit_expr(ts);
                }
                self.output.push(')');
            }
            Expr::BufferWrite { buffer, string, offset, .. } => {
                self.output.push_str("((() => { const _b = new TextEncoder().encode(");
                self.emit_expr(string);
                self.output.push_str("); ");
                self.emit_expr(buffer);
                self.output.push_str(".set(_b, ");
                if let Some(o) = offset { self.emit_expr(o); } else { self.output.push('0'); }
                self.output.push_str("); return _b.length; })())");
            }
            Expr::BufferEquals { buffer, other } => {
                self.output.push_str("((() => { const _a = ");
                self.emit_expr(buffer);
                self.output.push_str(", _b = ");
                self.emit_expr(other);
                self.output.push_str("; return _a.length === _b.length && _a.every((v, i) => v === _b[i]); })())");
            }
            Expr::BufferIndexGet { buffer, index } => {
                self.emit_expr(buffer);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push(']');
            }
            Expr::BufferIndexSet { buffer, index, value } => {
                self.output.push('(');
                self.emit_expr(buffer);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push_str("] = ");
                self.emit_expr(value);
                self.output.push(')');
            }

            // --- Typed arrays ---
            Expr::Uint8ArrayNew(size) => {
                self.output.push_str("new Uint8Array(");
                if let Some(s) = size { self.emit_expr(s); }
                self.output.push(')');
            }
            // NOTE: TypedArrayNew variant referenced an HIR variant that was
            // never landed; the corresponding lower.rs path was reverted.
            // Reinstating it requires landing the typedarray HIR work.
            Expr::Uint8ArrayFrom(src) => {
                self.output.push_str("Uint8Array.from(");
                self.emit_expr(src);
                self.output.push(')');
            }
            Expr::Uint8ArrayLength(arr) => {
                self.emit_expr(arr);
                self.output.push_str(".length");
            }
            Expr::Uint8ArrayGet { array, index } => {
                self.emit_expr(array);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push(']');
            }
            Expr::Uint8ArraySet { array, index, value } => {
                self.output.push('(');
                self.emit_expr(array);
                self.output.push('[');
                self.emit_expr(index);
                self.output.push_str("] = ");
                self.emit_expr(value);
                self.output.push(')');
            }
            // --- Continued in exprs_more.rs ---
            _ => self.emit_expr_continued(expr),
        }
    }
}
