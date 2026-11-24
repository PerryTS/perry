//! HIR-specialized member operations and their site records
//! (DESIGN-native-dispatch PR 1, NDISP2; ACCSITE1 "one record for every
//! member access").
//!
//! HIR lowers many member accesses `R.k(..)` to dedicated nodes
//! (`Expr::ArrayIndexOf`, `Expr::MathMax`, `console.log` as
//! `Expr::NativeMethodCall`, ...). Such a node has no `PropertyGet` and so no
//! named site of its own, yet it is a member access: each one carries a
//! record here ([`MemberOp`], published as [`MemberOpFacts`]) that states its
//! dispatcher obligation, the key and the receivers.
//!
//! - **Folded** ([`Op::Folded`], the `perry_hir::folded_call` table and the
//!   native array push): the node is the call `receiver.method(..)`. Codegen
//!   guards the fold; a receiver that is not the helper's own kind runs the
//!   generic call, and the runtime helper redispatches it into the generic
//!   method dispatcher. The record names the receiver node and says whether
//!   that fallback edge can be taken (`redispatch`).
//! - **Native** ([`Op::Native`], `Expr::NativeMethodCall`): a call of a
//!   native module's member, on a receiver (`object`, which may fall back
//!   into the dispatcher) or on the module namespace (a direct call).
//! - **Static** ([`Op::Static`]): a namespace member (`Math.max`,
//!   `Object.keys`, `JSON.parse`, ...), a direct call of the realm's
//!   builtin. What its arguments run (an implicit ToPrimitive or iteration
//!   on an object, a callback) are routes of their own. The key is always
//!   the real JS member name, and a member of a sub-object has the
//!   sub-object as its namespace (`process.stdout` / `columns`,
//!   `crypto.subtle` / `digest`, `path.win32` / `join`). "Static" means
//!   *direct, never reaches the tower*: that holds as well for a member
//!   that is read, not called (`process.argv`, `os.EOL`, `path.sep`,
//!   `process.stdout`), and for a setter (`process.title = v`). No node
//!   uses such a key as a Get-then-Call key (the dispatcher fact's
//!   obligation of a namespace record is a direct call), so there is no
//!   separate kind for them.
//! - **Construct** ([`Op::Construct`]): `new NS.Name(..)` of a realm
//!   builtin (`new WebAssembly.Module`): not a member call. The key is the
//!   constructor's name.
//! - **Internal** ([`Op::Internal`]): a node whose Perry-source name is no
//!   JS member (`crypto.randomUUIDv7`, `fs.rmRecursive`, the child_process
//!   background helpers). The record keeps the Perry name as its key and
//!   is marked [`MemberForm::Internal`]; no runtime method has that name.
//! - **Unstated** ([`Op::Unstated`]): a receiver operation the generation
//!   lowers through a shared arm, which names no receiver node of its own.
//!   Its record states no receivers, so the dispatcher fact is open.
//! - **Core**: a language node, a constructor or an element operation.
//!
//! The match has no wildcard arm on purpose: a new HIR node does not
//! compile until it is classified here.

use crate::model::{Atom, NodeId};
use perry_hir::Expr;

/// The member-op class of one HIR node (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Folded,
    Native,
    /// The namespace and the key.
    Static(&'static str, &'static str),
    /// The namespace and the constructor's name.
    Construct(&'static str, &'static str),
    /// The namespace and the Perry-internal name (no JS member).
    Internal(&'static str, &'static str),
    Unstated,
    Core,
}

/// What a namespace record states (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberForm {
    /// `key` is the real JS member name.
    Member,
    /// `new namespace.key(..)`.
    Construct,
    /// `key` is a Perry-internal name; there is no JS member.
    Internal,
}

/// The record of one HIR-specialized member op, as the generation makes it.
#[derive(Clone, Debug)]
pub(crate) struct MemberOp {
    pub module: u32,
    /// The HIR node's variant.
    pub op: String,
    pub key: Atom,
    /// The receiver node, when the op has a receiver the generation made.
    pub recv: Option<NodeId>,
    /// The namespace, for a namespace member.
    pub namespace: Option<String>,
    /// A folded node (`perry_hir::folded_call`): a receiver that is not the
    /// helper's own kind takes the generic call.
    pub folded: bool,
    /// Native push route, keyed exactly as the codegen method/fill proof.
    pub native_push: Option<perry_hir::folded_call::Site>,
    pub form: MemberForm,
}

/// The published record of a HIR-specialized member op: its obligation.
#[derive(Clone, Debug)]
pub struct MemberOpFacts {
    pub module: u32,
    /// The HIR node's variant (`ArrayIndexOf`, `MathMax`, ...).
    pub op: String,
    pub key: String,
    /// `Some`: a namespace member (`Math`, `console`): a direct call.
    pub namespace: Option<String>,
    /// The solved receiver values (unguarded, every summary copy).
    pub receiver_values: Vec<crate::ReceiverValue>,
    pub receiver_prims: u16,
    /// Whether the op states its receivers (a namespace, or a receiver
    /// node). `false` keeps the dispatcher fact open.
    pub stated: bool,
    /// The fallback edge: a receiver value the op's own path does not
    /// handle may reach the generic method dispatcher (Get-then-Call of
    /// `key` on it).
    pub redispatch: bool,
    pub form: MemberForm,
}

impl MemberOpFacts {
    /// The dump's line: `# member_op <module> <op> key= ns=|receivers=[..] prims= redispatch=0|1`.
    pub fn dump_line(&self) -> String {
        let recv = match (&self.namespace, self.stated) {
            (Some(ns), _) => format!("ns={ns}"),
            (None, true) => format!(
                "receivers=[{}] prims={}",
                self.receiver_values
                    .iter()
                    .map(|r| r.display.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                self.receiver_prims
            ),
            (None, false) => "receivers=unstated".to_owned(),
        };
        let form = match self.form {
            MemberForm::Member => "",
            MemberForm::Construct => " form=construct",
            MemberForm::Internal => " form=internal",
        };
        format!(
            "# member_op {} {} key={} {recv} redispatch={}{form}",
            self.module, self.op, self.key, self.redispatch as u8
        )
    }
}

#[rustfmt::skip]
pub(crate) fn class(e: &Expr) -> Op {
    match e {
        Expr::ArrayAt { .. }
        | Expr::ArrayCopyWithin { .. }
        | Expr::ArrayEntries { .. }
        | Expr::ArrayEvery { .. }
        | Expr::ArrayFilter { .. }
        | Expr::ArrayFind { .. }
        | Expr::ArrayFindIndex { .. }
        | Expr::ArrayFindLast { .. }
        | Expr::ArrayFindLastIndex { .. }
        | Expr::ArrayFlat { .. }
        | Expr::ArrayFlatMap { .. }
        | Expr::ArrayForEach { .. }
        | Expr::ArrayIncludes { .. }
        | Expr::ArrayIndexOf { .. }
        | Expr::ArrayJoin { .. }
        | Expr::ArrayKeys { .. }
        | Expr::ArrayLastIndexOf { .. }
        | Expr::ArrayMap { .. }
        | Expr::ArrayPush { .. }
        | Expr::ArrayReduce { .. }
        | Expr::ArrayReduceRight { .. }
        | Expr::ArrayShift { .. }
        | Expr::ArraySlice { .. }
        | Expr::ArraySome { .. }
        | Expr::ArraySort { .. }
        | Expr::ArraySplice { .. }
        | Expr::ArrayToReversed { .. }
        | Expr::ArrayToSorted { .. }
        | Expr::ArrayToSpliced { .. }
        | Expr::ArrayUnshift { .. }
        | Expr::ArrayValues { .. }
        | Expr::ArrayWith { .. }
        | Expr::DateGetDate { .. }
        | Expr::DateGetDay { .. }
        | Expr::DateGetFullYear { .. }
        | Expr::DateGetHours { .. }
        | Expr::DateGetMilliseconds { .. }
        | Expr::DateGetMinutes { .. }
        | Expr::DateGetMonth { .. }
        | Expr::DateGetSeconds { .. }
        | Expr::DateGetTime { .. }
        | Expr::DateGetTimezoneOffset { .. }
        | Expr::DateGetUtcDate { .. }
        | Expr::DateGetUtcDay { .. }
        | Expr::DateGetUtcFullYear { .. }
        | Expr::DateGetUtcHours { .. }
        | Expr::DateGetUtcMilliseconds { .. }
        | Expr::DateGetUtcMinutes { .. }
        | Expr::DateGetUtcMonth { .. }
        | Expr::DateGetUtcSeconds { .. }
        | Expr::DateSetDate { .. }
        | Expr::DateSetFullYear { .. }
        | Expr::DateSetHours { .. }
        | Expr::DateSetMilliseconds { .. }
        | Expr::DateSetMinutes { .. }
        | Expr::DateSetMonth { .. }
        | Expr::DateSetSeconds { .. }
        | Expr::DateSetTime { .. }
        | Expr::DateSetUtcDate { .. }
        | Expr::DateSetUtcFullYear { .. }
        | Expr::DateSetUtcHours { .. }
        | Expr::DateSetUtcMilliseconds { .. }
        | Expr::DateSetUtcMinutes { .. }
        | Expr::DateSetUtcMonth { .. }
        | Expr::DateSetUtcSeconds { .. }
        | Expr::DateToDateString { .. }
        | Expr::DateToISOString { .. }
        | Expr::DateToJSON { .. }
        | Expr::DateToLocaleDateString { .. }
        | Expr::DateToLocaleString { .. }
        | Expr::DateToLocaleTimeString { .. }
        | Expr::DateToTimeString { .. }
        | Expr::MapDelete { .. }
        | Expr::MapGet { .. }
        | Expr::MapHas { .. }
        | Expr::MapSet { .. }
        | Expr::SetAdd { .. }
        | Expr::SetDelete { .. }
        | Expr::SetHas { .. } => Op::Folded,
        Expr::NativeMethodCall { .. } => Op::Native,
        Expr::ArrayFrom { .. } => Op::Static("Array", "from"),
        Expr::ArrayFromArrayLikeHoley { .. } => Op::Static("Array", "from"),
        Expr::ArrayFromMapped { .. } => Op::Static("Array", "from"),
        Expr::ArrayIsArray { .. } => Op::Static("Array", "isArray"),
        Expr::Atob { .. } => Op::Static("globalThis", "atob"),
        Expr::Btoa { .. } => Op::Static("globalThis", "btoa"),
        Expr::BufferAlloc { .. } => Op::Static("Buffer", "alloc"),
        Expr::BufferAllocUnsafe { .. } => Op::Static("Buffer", "allocUnsafe"),
        Expr::BufferByteLength { .. } => Op::Static("Buffer", "byteLength"),
        Expr::BufferConcat { .. } => Op::Static("Buffer", "concat"),
        Expr::BufferConcatWithLength { .. } => Op::Static("Buffer", "concat"),
        Expr::BufferFrom { .. } => Op::Static("Buffer", "from"),
        Expr::BufferFromArrayBuffer { .. } => Op::Static("Buffer", "from"),
        Expr::BufferIsBuffer { .. } => Op::Static("Buffer", "isBuffer"),
        Expr::BufferIsEncoding { .. } => Op::Static("Buffer", "isEncoding"),
        Expr::ChildProcessExec { .. } => Op::Static("child_process", "exec"),
        Expr::ChildProcessExecFile { .. } => Op::Static("child_process", "execFile"),
        Expr::ChildProcessExecFileSync { .. } => Op::Static("child_process", "execFileSync"),
        Expr::ChildProcessExecSync { .. } => Op::Static("child_process", "execSync"),
        Expr::ChildProcessFork { .. } => Op::Static("child_process", "fork"),
        Expr::ChildProcessGetProcessStatus { .. } => Op::Internal("child_process", "getProcessStatus"),
        Expr::ChildProcessKillProcess { .. } => Op::Internal("child_process", "killProcess"),
        Expr::ChildProcessSpawn { .. } => Op::Static("child_process", "spawn"),
        Expr::ChildProcessSpawnBackground { .. } => Op::Internal("child_process", "spawnBackground"),
        Expr::ChildProcessSpawnSync { .. } => Op::Static("child_process", "spawnSync"),
        Expr::CryptoMd5 { .. } => Op::Internal("crypto", "md5"),
        Expr::CryptoRandomBytes { .. } => Op::Static("crypto", "randomBytes"),
        Expr::CryptoRandomFillSync { .. } => Op::Static("crypto", "randomFillSync"),
        Expr::CryptoRandomUUID { .. } => Op::Static("crypto", "randomUUID"),
        Expr::CryptoRandomUUIDv7 { .. } => Op::Internal("crypto", "randomUUIDv7"),
        Expr::CryptoSha256 { .. } => Op::Internal("crypto", "sha256"),
        Expr::DateNow { .. } => Op::Static("Date", "now"),
        Expr::DateParse { .. } => Op::Static("Date", "parse"),
        Expr::DateUtc { .. } => Op::Static("Date", "UTC"),
        Expr::DecodeURI { .. } => Op::Static("globalThis", "decodeURI"),
        Expr::DecodeURIComponent { .. } => Op::Static("globalThis", "decodeURIComponent"),
        Expr::EncodeURI { .. } => Op::Static("globalThis", "encodeURI"),
        Expr::EncodeURIComponent { .. } => Op::Static("globalThis", "encodeURIComponent"),
        Expr::FetchGetWithAuth { .. } => Op::Static("globalThis", "fetch"),
        Expr::FetchPostWithAuth { .. } => Op::Static("globalThis", "fetch"),
        Expr::FetchWithOptions { .. } => Op::Static("globalThis", "fetch"),
        Expr::FileURLToPath { .. } => Op::Static("url", "fileURLToPath"),
        Expr::FsAppendFileSync { .. } => Op::Static("fs", "appendFileSync"),
        Expr::FsExistsSync { .. } => Op::Static("fs", "existsSync"),
        Expr::FsMkdirSync { .. } => Op::Static("fs", "mkdirSync"),
        Expr::FsReadFileBinary { .. } => Op::Static("fs", "readFileSync"),
        Expr::FsReadFileSync { .. } => Op::Static("fs", "readFileSync"),
        Expr::FsRmRecursive { .. } => Op::Internal("fs", "rmRecursive"),
        Expr::FsUnlinkSync { .. } => Op::Static("fs", "unlinkSync"),
        Expr::FsWriteFileSync { .. } => Op::Static("fs", "writeFileSync"),
        Expr::IsFinite { .. } => Op::Static("globalThis", "isFinite"),
        Expr::IsNaN { .. } => Op::Static("globalThis", "isNaN"),
        Expr::IteratorFrom { .. } => Op::Static("Iterator", "from"),
        Expr::JsonIsRawJson { .. } => Op::Static("JSON", "isRawJson"),
        Expr::JsonParse { .. } => Op::Static("JSON", "parse"),
        Expr::JsonParseReviver { .. } => Op::Static("JSON", "parse"),
        Expr::JsonParseTyped { .. } => Op::Static("JSON", "parse"),
        Expr::JsonParseWithReviver { .. } => Op::Static("JSON", "parse"),
        Expr::JsonRawJson { .. } => Op::Static("JSON", "rawJson"),
        Expr::JsonStringify { .. } => Op::Static("JSON", "stringify"),
        Expr::JsonStringifyFull { .. } => Op::Static("JSON", "stringify"),
        Expr::JsonStringifyPretty { .. } => Op::Static("JSON", "stringify"),
        Expr::MapGroupBy { .. } => Op::Static("Map", "groupBy"),
        Expr::MathAbs { .. } => Op::Static("Math", "abs"),
        Expr::MathAcos { .. } => Op::Static("Math", "acos"),
        Expr::MathAcosh { .. } => Op::Static("Math", "acosh"),
        Expr::MathAsin { .. } => Op::Static("Math", "asin"),
        Expr::MathAsinh { .. } => Op::Static("Math", "asinh"),
        Expr::MathAtan { .. } => Op::Static("Math", "atan"),
        Expr::MathAtan2 { .. } => Op::Static("Math", "atan2"),
        Expr::MathAtanh { .. } => Op::Static("Math", "atanh"),
        Expr::MathCbrt { .. } => Op::Static("Math", "cbrt"),
        Expr::MathCeil { .. } => Op::Static("Math", "ceil"),
        Expr::MathClz32 { .. } => Op::Static("Math", "clz32"),
        Expr::MathCos { .. } => Op::Static("Math", "cos"),
        Expr::MathCosh { .. } => Op::Static("Math", "cosh"),
        Expr::MathExp { .. } => Op::Static("Math", "exp"),
        Expr::MathExpm1 { .. } => Op::Static("Math", "expm1"),
        Expr::MathF16round { .. } => Op::Static("Math", "f16round"),
        Expr::MathFloor { .. } => Op::Static("Math", "floor"),
        Expr::MathFround { .. } => Op::Static("Math", "fround"),
        Expr::MathHypot { .. } => Op::Static("Math", "hypot"),
        Expr::MathImul { .. } => Op::Static("Math", "imul"),
        Expr::MathLog { .. } => Op::Static("Math", "log"),
        Expr::MathLog10 { .. } => Op::Static("Math", "log10"),
        Expr::MathLog1p { .. } => Op::Static("Math", "log1p"),
        Expr::MathLog2 { .. } => Op::Static("Math", "log2"),
        Expr::MathMax { .. } => Op::Static("Math", "max"),
        Expr::MathMaxSpread { .. } => Op::Static("Math", "max"),
        Expr::MathMin { .. } => Op::Static("Math", "min"),
        Expr::MathMinSpread { .. } => Op::Static("Math", "min"),
        Expr::MathPow { .. } => Op::Static("Math", "pow"),
        Expr::MathRandom { .. } => Op::Static("Math", "random"),
        Expr::MathRound { .. } => Op::Static("Math", "round"),
        Expr::MathSign { .. } => Op::Static("Math", "sign"),
        Expr::MathSin { .. } => Op::Static("Math", "sin"),
        Expr::MathSinh { .. } => Op::Static("Math", "sinh"),
        Expr::MathSqrt { .. } => Op::Static("Math", "sqrt"),
        Expr::MathTan { .. } => Op::Static("Math", "tan"),
        Expr::MathTanh { .. } => Op::Static("Math", "tanh"),
        Expr::MathTrunc { .. } => Op::Static("Math", "trunc"),
        Expr::NetConnect { .. } => Op::Static("net", "connect"),
        Expr::NetCreateConnection { .. } => Op::Static("net", "createConnection"),
        Expr::NetCreateServer { .. } => Op::Static("net", "createServer"),
        Expr::NumberIsFinite { .. } => Op::Static("Number", "isFinite"),
        Expr::NumberIsInteger { .. } => Op::Static("Number", "isInteger"),
        Expr::NumberIsNaN { .. } => Op::Static("Number", "isNaN"),
        Expr::NumberIsSafeInteger { .. } => Op::Static("Number", "isSafeInteger"),
        Expr::ObjectAssign { .. } => Op::Static("Object", "assign"),
        Expr::ObjectCreate { .. } => Op::Static("Object", "create"),
        Expr::ObjectDefineProperties { .. } => Op::Static("Object", "defineProperties"),
        Expr::ObjectDefineProperty { .. } => Op::Static("Object", "defineProperty"),
        Expr::ObjectEntries { .. } => Op::Static("Object", "entries"),
        Expr::ObjectFreeze { .. } => Op::Static("Object", "freeze"),
        Expr::ObjectFromEntries { .. } => Op::Static("Object", "fromEntries"),
        Expr::ObjectGetOwnPropertyDescriptor { .. } => Op::Static("Object", "getOwnPropertyDescriptor"),
        Expr::ObjectGetOwnPropertyDescriptors { .. } => Op::Static("Object", "getOwnPropertyDescriptors"),
        Expr::ObjectGetOwnPropertyNames { .. } => Op::Static("Object", "getOwnPropertyNames"),
        Expr::ObjectGetOwnPropertySymbols { .. } => Op::Static("Object", "getOwnPropertySymbols"),
        Expr::ObjectGetPrototypeOf { .. } => Op::Static("Object", "getPrototypeOf"),
        Expr::ObjectGroupBy { .. } => Op::Static("Object", "groupBy"),
        Expr::ObjectHasOwn { .. } => Op::Static("Object", "hasOwn"),
        Expr::ObjectIs { .. } => Op::Static("Object", "is"),
        Expr::ObjectIsExtensible { .. } => Op::Static("Object", "isExtensible"),
        Expr::ObjectIsFrozen { .. } => Op::Static("Object", "isFrozen"),
        Expr::ObjectIsSealed { .. } => Op::Static("Object", "isSealed"),
        Expr::ObjectKeys { .. } => Op::Static("Object", "keys"),
        Expr::ObjectPreventExtensions { .. } => Op::Static("Object", "preventExtensions"),
        Expr::ObjectSeal { .. } => Op::Static("Object", "seal"),
        Expr::ObjectSetPrototypeOf { .. } => Op::Static("Object", "setPrototypeOf"),
        Expr::ObjectValues { .. } => Op::Static("Object", "values"),
        Expr::OsArch { .. } => Op::Static("os", "arch"),
        Expr::OsAvailableParallelism { .. } => Op::Static("os", "availableParallelism"),
        Expr::OsCpus { .. } => Op::Static("os", "cpus"),
        Expr::OsDevNull { .. } => Op::Static("os", "devNull"),
        Expr::OsEndianness { .. } => Op::Static("os", "endianness"),
        Expr::OsEOL { .. } => Op::Static("os", "EOL"),
        Expr::OsFreemem { .. } => Op::Static("os", "freemem"),
        Expr::OsHomedir { .. } => Op::Static("os", "homedir"),
        Expr::OsHostname { .. } => Op::Static("os", "hostname"),
        Expr::OsLoadavg { .. } => Op::Static("os", "loadavg"),
        Expr::OsMachine { .. } => Op::Static("os", "machine"),
        Expr::OsNetworkInterfaces { .. } => Op::Static("os", "networkInterfaces"),
        Expr::OsPlatform { .. } => Op::Static("os", "platform"),
        Expr::OsRelease { .. } => Op::Static("os", "release"),
        Expr::OsTmpdir { .. } => Op::Static("os", "tmpdir"),
        Expr::OsTotalmem { .. } => Op::Static("os", "totalmem"),
        Expr::OsType { .. } => Op::Static("os", "type"),
        Expr::OsUptime { .. } => Op::Static("os", "uptime"),
        Expr::OsUserInfo { .. } => Op::Static("os", "userInfo"),
        Expr::OsUserInfoBuffer { .. } => Op::Static("os", "userInfo"),
        Expr::OsVersion { .. } => Op::Static("os", "version"),
        Expr::ParseFloat { .. } => Op::Static("globalThis", "parseFloat"),
        Expr::ParseInt { .. } => Op::Static("globalThis", "parseInt"),
        Expr::PathBasename { .. } => Op::Static("path", "basename"),
        Expr::PathBasenameExt { .. } => Op::Static("path", "basename"),
        Expr::PathDelimiter { .. } => Op::Static("path", "delimiter"),
        Expr::PathDirname { .. } => Op::Static("path", "dirname"),
        Expr::PathExtname { .. } => Op::Static("path", "extname"),
        Expr::PathFormat { .. } => Op::Static("path", "format"),
        Expr::PathIsAbsolute { .. } => Op::Static("path", "isAbsolute"),
        Expr::PathJoin { .. } => Op::Static("path", "join"),
        Expr::PathMatchesGlob { .. } => Op::Static("path", "matchesGlob"),
        Expr::PathNormalize { .. } => Op::Static("path", "normalize"),
        Expr::PathParse { .. } => Op::Static("path", "parse"),
        Expr::PathRelative { .. } => Op::Static("path", "relative"),
        Expr::PathResolve { .. } => Op::Static("path", "resolve"),
        Expr::PathResolveJoin { .. } => Op::Static("path", "resolve"),
        Expr::PathSep { .. } => Op::Static("path", "sep"),
        Expr::PathToNamespacedPath { .. } => Op::Static("path", "toNamespacedPath"),
        Expr::PathWin32 { .. } => Op::Static("path", "win32"),
        Expr::PathWin32Join { .. } => Op::Static("path.win32", "join"),
        Expr::PerformanceNow { .. } => Op::Static("performance", "now"),
        Expr::ProcessAbort { .. } => Op::Static("process", "abort"),
        Expr::ProcessActiveResourcesInfo { .. } => Op::Static("process", "getActiveResourcesInfo"),
        Expr::ProcessArgv { .. } => Op::Static("process", "argv"),
        Expr::ProcessAvailableMemory { .. } => Op::Static("process", "availableMemory"),
        Expr::ProcessChdir { .. } => Op::Static("process", "chdir"),
        Expr::ProcessConstrainedMemory { .. } => Op::Static("process", "constrainedMemory"),
        Expr::ProcessCpuUsage { .. } => Op::Static("process", "cpuUsage"),
        Expr::ProcessCwd { .. } => Op::Static("process", "cwd"),
        Expr::ProcessEmitWarning { .. } => Op::Static("process", "emitWarning"),
        Expr::ProcessEnv { .. } => Op::Static("process", "env"),
        Expr::ProcessExit { .. } => Op::Static("process", "exit"),
        Expr::ProcessHrtime { .. } => Op::Static("process", "hrtime"),
        Expr::ProcessHrtimeBigint { .. } => Op::Static("process.hrtime", "bigint"),
        Expr::ProcessKill { .. } => Op::Static("process", "kill"),
        Expr::ProcessMemoryUsage { .. } => Op::Static("process", "memoryUsage"),
        Expr::ProcessNextTick { .. } => Op::Static("process", "nextTick"),
        Expr::ProcessOn { .. } => Op::Static("process", "on"),
        Expr::ProcessOnce { .. } => Op::Static("process", "once"),
        Expr::ProcessPid { .. } => Op::Static("process", "pid"),
        Expr::ProcessPosixCredential(k) => Op::Static("process", match k {
            perry_hir::PosixCredentialKind::Uid => "getuid",
            perry_hir::PosixCredentialKind::Euid => "geteuid",
            perry_hir::PosixCredentialKind::Gid => "getgid",
            perry_hir::PosixCredentialKind::Egid => "getegid",
        }),
        Expr::ProcessPpid { .. } => Op::Static("process", "ppid"),
        Expr::ProcessResourceUsage { .. } => Op::Static("process", "resourceUsage"),
        Expr::ProcessSetTitle { .. } => Op::Static("process", "title"),
        Expr::ProcessStderr { .. } => Op::Static("process", "stderr"),
        Expr::ProcessStderrIsTTY { .. } => Op::Static("process.stderr", "isTTY"),
        Expr::ProcessStdin { .. } => Op::Static("process", "stdin"),
        Expr::ProcessStdinIsTTY { .. } => Op::Static("process.stdin", "isTTY"),
        Expr::ProcessStdinLifecycle { .. } => Op::Internal("process.stdin", "stdinLifecycle"),
        Expr::ProcessStdinOn { .. } => Op::Static("process.stdin", "on"),
        Expr::ProcessStdinRemoveListener { .. } => Op::Static("process.stdin", "removeListener"),
        Expr::ProcessStdinSetRawMode { .. } => Op::Static("process.stdin", "setRawMode"),
        Expr::ProcessStdout { .. } => Op::Static("process", "stdout"),
        Expr::ProcessStdoutColumns { .. } => Op::Static("process.stdout", "columns"),
        Expr::ProcessStdoutIsTTY { .. } => Op::Static("process.stdout", "isTTY"),
        Expr::ProcessStdoutOn { .. } => Op::Static("process.stdout", "on"),
        Expr::ProcessStdoutRows { .. } => Op::Static("process.stdout", "rows"),
        Expr::ProcessThreadCpuUsage { .. } => Op::Static("process", "threadCpuUsage"),
        Expr::ProcessTitle { .. } => Op::Static("process", "title"),
        Expr::ProcessUmask { .. } => Op::Static("process", "umask"),
        Expr::ProcessUptime { .. } => Op::Static("process", "uptime"),
        Expr::ProcessVersion { .. } => Op::Static("process", "version"),
        Expr::ProcessVersions { .. } => Op::Static("process", "versions"),
        Expr::QueueMicrotask { .. } => Op::Static("globalThis", "queueMicrotask"),
        Expr::ReflectApply { .. } => Op::Static("Reflect", "apply"),
        Expr::ReflectConstruct { .. } => Op::Static("Reflect", "construct"),
        Expr::ReflectDefineMetadata { .. } => Op::Static("Reflect", "defineMetadata"),
        Expr::ReflectDefineProperty { .. } => Op::Static("Reflect", "defineProperty"),
        Expr::ReflectDelete { .. } => Op::Static("Reflect", "delete"),
        Expr::ReflectDeleteMetadata { .. } => Op::Static("Reflect", "deleteMetadata"),
        Expr::ReflectGet { .. } => Op::Static("Reflect", "get"),
        Expr::ReflectGetMetadata { .. } => Op::Static("Reflect", "getMetadata"),
        Expr::ReflectGetMetadataKeys { .. } => Op::Static("Reflect", "getMetadataKeys"),
        Expr::ReflectGetOwnMetadata { .. } => Op::Static("Reflect", "getOwnMetadata"),
        Expr::ReflectGetOwnMetadataKeys { .. } => Op::Static("Reflect", "getOwnMetadataKeys"),
        Expr::ReflectGetOwnPropertyDescriptor { .. } => Op::Static("Reflect", "getOwnPropertyDescriptor"),
        Expr::ReflectGetPrototypeOf { .. } => Op::Static("Reflect", "getPrototypeOf"),
        Expr::ReflectHas { .. } => Op::Static("Reflect", "has"),
        Expr::ReflectHasMetadata { .. } => Op::Static("Reflect", "hasMetadata"),
        Expr::ReflectHasOwnMetadata { .. } => Op::Static("Reflect", "hasOwnMetadata"),
        Expr::ReflectIsExtensible { .. } => Op::Static("Reflect", "isExtensible"),
        Expr::ReflectOwnKeys { .. } => Op::Static("Reflect", "ownKeys"),
        Expr::ReflectPreventExtensions { .. } => Op::Static("Reflect", "preventExtensions"),
        Expr::ReflectSet { .. } => Op::Static("Reflect", "set"),
        Expr::ReflectSetPrototypeOf { .. } => Op::Static("Reflect", "setPrototypeOf"),
        Expr::RegExpEscape { .. } => Op::Static("RegExp", "escape"),
        Expr::StringFromCharCode { .. } => Op::Static("String", "fromCharCode"),
        Expr::StringFromCharCodeSpread { .. } => Op::Static("String", "fromCharCode"),
        Expr::StringFromCodePoint { .. } => Op::Static("String", "fromCodePoint"),
        Expr::StringRaw { .. } => Op::Static("String", "raw"),
        Expr::StructuredClone { .. } => Op::Static("globalThis", "structuredClone"),
        Expr::SymbolFor { .. } => Op::Static("Symbol", "for"),
        Expr::SymbolKeyFor { .. } => Op::Static("Symbol", "keyFor"),
        Expr::TtyIsAtty { .. } => Op::Static("tty", "isatty"),
        Expr::Uint8ArrayFrom { .. } => Op::Static("Uint8Array", "from"),
        Expr::UrlCanParse { .. } => Op::Static("URL", "canParse"),
        Expr::UrlCanParseWithBase { .. } => Op::Static("URL", "canParse"),
        Expr::UrlParse { .. } => Op::Static("URL", "parse"),
        Expr::UrlParseWithBase { .. } => Op::Static("URL", "parse"),
        Expr::WebAssemblyCompile { .. } => Op::Static("WebAssembly", "compile"),
        Expr::WebAssemblyInstantiate { .. } => Op::Static("WebAssembly", "instantiate"),
        Expr::WebAssemblyModuleCustomSections { .. } => Op::Static("WebAssembly.Module", "customSections"),
        Expr::WebAssemblyModuleExports { .. } => Op::Static("WebAssembly.Module", "exports"),
        Expr::WebAssemblyModuleImports { .. } => Op::Static("WebAssembly.Module", "imports"),
        Expr::WebAssemblyModuleNew { .. } => Op::Construct("WebAssembly", "Module"),
        Expr::WebAssemblyValidate { .. } => Op::Static("WebAssembly", "validate"),
        Expr::WebCryptoDecrypt { .. } => Op::Static("crypto.subtle", "decrypt"),
        Expr::WebCryptoDeriveBits { .. } => Op::Static("crypto.subtle", "deriveBits"),
        Expr::WebCryptoDeriveKey { .. } => Op::Static("crypto.subtle", "deriveKey"),
        Expr::WebCryptoDigest { .. } => Op::Static("crypto.subtle", "digest"),
        Expr::WebCryptoEncrypt { .. } => Op::Static("crypto.subtle", "encrypt"),
        Expr::WebCryptoExportKey { .. } => Op::Static("crypto.subtle", "exportKey"),
        Expr::WebCryptoGenerateKey { .. } => Op::Static("crypto.subtle", "generateKey"),
        Expr::WebCryptoImportKey { .. } => Op::Static("crypto.subtle", "importKey"),
        Expr::WebCryptoSign { .. } => Op::Static("crypto.subtle", "sign"),
        Expr::WebCryptoUnwrapKey { .. } => Op::Static("crypto.subtle", "unwrapKey"),
        Expr::WebCryptoVerify { .. } => Op::Static("crypto.subtle", "verify"),
        Expr::WebCryptoWrapKey { .. } => Op::Static("crypto.subtle", "wrapKey"),
        Expr::ArrayCopyWithinValue { .. }
        | Expr::ArrayPop { .. }
        | Expr::ArrayPushSpread { .. }
        | Expr::ArrayReverseValue { .. }
        | Expr::BufferCopy { .. }
        | Expr::BufferEquals { .. }
        | Expr::BufferFill { .. }
        | Expr::BufferSlice { .. }
        | Expr::BufferToString { .. }
        | Expr::BufferWrite { .. }
        | Expr::DateToString { .. }
        | Expr::DateToUTCString { .. }
        | Expr::DateValueOf { .. }
        | Expr::ErrorMessage { .. }
        | Expr::FinalizationRegistryRegister { .. }
        | Expr::FinalizationRegistryUnregister { .. }
        | Expr::MapClear { .. }
        | Expr::MapEntries { .. }
        | Expr::MapKeys { .. }
        | Expr::MapSize { .. }
        | Expr::MapValues { .. }
        | Expr::RegExpExecGroups { .. }
        | Expr::RegExpExecIndex { .. }
        | Expr::SetClear { .. }
        | Expr::SetSize { .. }
        | Expr::SetValues { .. }
        | Expr::StringAt { .. }
        | Expr::StringCodePointAt { .. }
        | Expr::StringMatch { .. }
        | Expr::StringMatchAll { .. }
        | Expr::StringReplace { .. }
        | Expr::StringSplit { .. }
        | Expr::SymbolDescription { .. }
        | Expr::SymbolToString { .. }
        | Expr::TextDecoderDecode { .. }
        | Expr::TextDecoderEncoding { .. }
        | Expr::TextDecoderFatal { .. }
        | Expr::TextDecoderIgnoreBom { .. }
        | Expr::TextEncoderEncode { .. }
        | Expr::TextEncoderEncodeInto { .. }
        | Expr::UrlGetHash { .. }
        | Expr::UrlGetHost { .. }
        | Expr::UrlGetHostname { .. }
        | Expr::UrlGetHref { .. }
        | Expr::UrlGetOrigin { .. }
        | Expr::UrlGetPathname { .. }
        | Expr::UrlGetPort { .. }
        | Expr::UrlGetProtocol { .. }
        | Expr::UrlGetSearch { .. }
        | Expr::UrlGetSearchParams { .. }
        | Expr::UrlInstanceToJSON { .. }
        | Expr::UrlInstanceToString { .. }
        | Expr::UrlSearchParamsAppend { .. }
        | Expr::UrlSearchParamsDelete { .. }
        | Expr::UrlSearchParamsEntries { .. }
        | Expr::UrlSearchParamsForEach { .. }
        | Expr::UrlSearchParamsGet { .. }
        | Expr::UrlSearchParamsGetAll { .. }
        | Expr::UrlSearchParamsHas { .. }
        | Expr::UrlSearchParamsKeys { .. }
        | Expr::UrlSearchParamsSet { .. }
        | Expr::UrlSearchParamsSort { .. }
        | Expr::UrlSearchParamsToString { .. }
        | Expr::UrlSearchParamsValues { .. }
        | Expr::UrlSetHash { .. }
        | Expr::UrlSetHostname { .. }
        | Expr::UrlSetHref { .. }
        | Expr::UrlSetPassword { .. }
        | Expr::UrlSetPathname { .. }
        | Expr::UrlSetPort { .. }
        | Expr::UrlSetProtocol { .. }
        | Expr::UrlSetSearch { .. }
        | Expr::UrlSetUsername { .. }
        | Expr::WeakRefDeref { .. }
        | Expr::WebAssemblyCallExport { .. } => Op::Unstated,
        Expr::AggregateErrorNew { .. }
        | Expr::Array { .. }
        | Expr::ArrayIterationPatched { .. }
        | Expr::ArrayLikeMethod { .. }
        | Expr::ArraySpread { .. }
        | Expr::AsyncFirstCall { .. }
        | Expr::AsyncGenResume { .. }
        | Expr::AsyncStepChain { .. }
        | Expr::AsyncStepDeclined { .. }
        | Expr::AsyncStepDone { .. }
        | Expr::AsyncStepResumeNow { .. }
        | Expr::Await { .. }
        | Expr::BigInt { .. }
        | Expr::BigIntCoerce { .. }
        | Expr::Binary { .. }
        | Expr::Bool { .. }
        | Expr::BooleanCoerce { .. }
        | Expr::BoxedPrimitiveNew { .. }
        | Expr::BufferIndexGet { .. }
        | Expr::BufferIndexSet { .. }
        | Expr::BufferLength { .. }
        | Expr::Call { .. } | Expr::RegionCall { .. }
        | Expr::CallSpread { .. }
        | Expr::ClassCaptureValue { .. }
        | Expr::ClassEnvCurrent { .. }
        | Expr::ClassEnvGet { .. }
        | Expr::ClassEnvSet { .. }
        | Expr::ClassEnvStamp { .. }
        | Expr::ClassExprFresh { .. }
        | Expr::ClassIsFirstEvaluation { .. }
        | Expr::ClassMethodHome { .. }
        | Expr::ClassRef { .. }
        | Expr::ClassStaticSymbolSet { .. }
        | Expr::Closure { .. }
        | Expr::Compare { .. }
        | Expr::Conditional { .. }
        | Expr::CurrentStepClosure { .. }
        | Expr::DateNew { .. }
        | Expr::Delete { .. }
        | Expr::DynamicImport { .. }
        | Expr::EnumImage { .. }
        | Expr::EnumMember { .. }
        | Expr::EnvGet { .. }
        | Expr::EnvGetDynamic { .. }
        | Expr::ErrorNew { .. }
        | Expr::ErrorNewWithCause { .. }
        | Expr::ErrorNewWithOptions { .. }
        | Expr::ExternFuncRef { .. }
        | Expr::FinalizationRegistryNew { .. }
        | Expr::ForAwaitToArray { .. }
        | Expr::ForInKeys { .. }
        | Expr::ForOfToArray { .. }
        | Expr::FuncRef { .. }
        | Expr::GetAsyncIterator { .. }
        | Expr::GetFunctionPrototypeMethod { .. }
        | Expr::GetIterator { .. }
        | Expr::GlobalGet { .. }
        | Expr::GlobalSet { .. }
        | Expr::GlobalThisExpr { .. }
        | Expr::I18nString { .. }
        | Expr::ImportMetaUrl { .. }
        | Expr::In { .. }
        | Expr::IndexGet { .. } | Expr::RegionOut { .. }
        | Expr::IndexSet { .. }
        | Expr::IndexUpdate { .. }
        | Expr::InstanceOf { .. }
        | Expr::Integer { .. }
        | Expr::IsUndefinedOrBareNan { .. }
        | Expr::IteratorToArray { .. }
        | Expr::IterResultGetDone { .. }
        | Expr::IterResultGetValue { .. }
        | Expr::IterResultSet { .. }
        | Expr::JsCallFunction { .. }
        | Expr::JsCallMethod { .. }
        | Expr::JsCallValue { .. }
        | Expr::JsCreateCallback { .. }
        | Expr::JsGetExport { .. }
        | Expr::JsGetProperty { .. }
        | Expr::JsLoadModule { .. }
        | Expr::JsNew { .. }
        | Expr::JsNewFromHandle { .. }
        | Expr::JsSetProperty { .. }
        | Expr::LinkGeneratorPrototype { .. }
        | Expr::LiteralChunkObject { .. }
        | Expr::LiteralChunks { .. }
        | Expr::LiteralSlotInit { .. }
        | Expr::LocalGet { .. }
        | Expr::LocalSet { .. }
        | Expr::Logical { .. }
        | Expr::MapEntryKeyAt { .. }
        | Expr::MapEntryValueAt { .. }
        | Expr::MapNew { .. }
        | Expr::MapNewFromArray { .. }
        | Expr::ModuleTopThis { .. }
        | Expr::NamespacePatched { .. }
        | Expr::NativeArenaAlloc { .. }
        | Expr::NativeArenaDispose { .. }
        | Expr::NativeArenaView { .. }
        | Expr::NativeMemoryCopy { .. }
        | Expr::NativeMemoryFillU32 { .. }
        | Expr::NativeModuleRef { .. }
        | Expr::NativePodView { .. }
        | Expr::New { .. }
        | Expr::NewDynamic { .. }
        | Expr::NewDynamicSpread { .. }
        | Expr::NewTarget { .. }
        | Expr::Null { .. }
        | Expr::Number { .. }
        | Expr::NumberCoerce { .. }
        | Expr::Object { .. }
        | Expr::ObjectCoerce { .. }
        | Expr::ObjectRest { .. }
        | Expr::ObjectSpread { .. }
        | Expr::ObjectSuperMethodCall { .. }
        | Expr::ObjectSuperPropertyGet { .. }
        | Expr::ObjectSuperPropertySet { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. }
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PrivateBrandCheck { .. }
        | Expr::PrivateGuard { .. }
        | Expr::PrivateLexicalBrand { .. }
        | Expr::PrivateLexicalBrandPop { .. }
        | Expr::PrivateLexicalBrandPush { .. }
        | Expr::PropertyGet { .. }
        | Expr::PropertySet { .. }
        | Expr::PropertyUpdate { .. }
        | Expr::ProxyApply { .. }
        | Expr::ProxyConstruct { .. }
        | Expr::ProxyDelete { .. }
        | Expr::ProxyGet { .. }
        | Expr::ProxyHas { .. }
        | Expr::ProxyNew { .. }
        | Expr::ProxyRevocable { .. }
        | Expr::ProxyRevoke { .. }
        | Expr::ProxySet { .. }
        | Expr::PutValueSet { .. }
        | Expr::RangeErrorNew { .. }
        | Expr::ReferenceErrorNew { .. }
        | Expr::RefreshClassExprCaptures { .. }
        | Expr::RegExp { .. }
        | Expr::RegExpDynamic { .. }
        | Expr::RegisterClassCaptures { .. }
        | Expr::RegisterClassComputedAccessor { .. }
        | Expr::RegisterClassComputedMethod { .. }
        | Expr::RegisterClassParentDynamic { .. }
        | Expr::RegisterClassStaticSymbol { .. }
        | Expr::RegisterFunctionPrototypeMethod { .. }
        | Expr::RegisterPrototypeMethod { .. }
        | Expr::ScopedTemp { .. }
        | Expr::Sequence { .. }
        | Expr::SetFunctionPrototype { .. }
        | Expr::SetNew { .. }
        | Expr::SetNewFromArray { .. }
        | Expr::SetValueAt { .. }
        | Expr::StaticFieldGet { .. }
        | Expr::StaticFieldSet { .. }
        | Expr::StaticMethodCall { .. }
        | Expr::StaticPluginResolve { .. }
        | Expr::String { .. }
        | Expr::StringCoerce { .. }
        | Expr::SuperCall { .. }
        | Expr::SuperCallSpread { .. }
        | Expr::SuperMethodCall { .. }
        | Expr::SuperMethodCallSpread { .. }
        | Expr::SuperPropertyGet { .. }
        | Expr::SuperPropertySet { .. }
        | Expr::SymbolNew { .. }
        | Expr::SyntaxErrorNew { .. }
        | Expr::TaggedTemplateStrings { .. }
        | Expr::TemplateRaw { .. }
        | Expr::TemplateStringCoerce { .. }
        | Expr::TextDecoderNew { .. }
        | Expr::TextEncoderNew { .. }
        | Expr::This { .. }
        | Expr::ThisProvenClass { .. }
        | Expr::TypedArrayNew { .. }
        | Expr::TypeErrorNew { .. }
        | Expr::TypeOf { .. }
        | Expr::Uint8ArrayGet { .. }
        | Expr::Uint8ArrayLength { .. }
        | Expr::Uint8ArrayNew { .. }
        | Expr::Uint8ArraySet { .. }
        | Expr::Unary { .. }
        | Expr::Undefined { .. }
        | Expr::Update { .. }
        | Expr::UrlNew { .. }
        | Expr::UrlPatternNew { .. }
        | Expr::UrlSearchParamsMissingArgs { .. }
        | Expr::UrlSearchParamsNew { .. }
        | Expr::Void { .. }
        | Expr::WeakRefNew { .. }
        | Expr::WithGet { .. }
        | Expr::WithSet { .. }
        | Expr::WorkerNew { .. }
        | Expr::WtfString { .. }
        | Expr::Yield { .. } => Op::Core,
    }
}

impl crate::gen::Gen<'_> {
    /// Record a member op (module docs).
    pub(crate) fn member_op(
        &mut self,
        module: u32,
        e: &Expr,
        key: &str,
        recv: Option<NodeId>,
        namespace: Option<&str>,
        folded: bool,
    ) {
        let key = self.m.atom(key);
        self.member_ops.push(MemberOp {
            module,
            op: crate::gen::variant_name(e),
            key,
            recv,
            namespace: namespace.map(str::to_owned),
            folded,
            native_push: perry_hir::folded_call::native_array_push(e)
                .and_then(|c| perry_hir::folded_call::receiver_site(&c.receiver)),
            form: MemberForm::Member,
        });
    }

    /// The record of a namespace member or of an op that states no
    /// receiver, after the node is generated. A folded node and a native
    /// call are recorded where their receiver is made: that record is the
    /// last one the node's generation pushed (`since`: the count before it).
    /// A receiver op whose arm made no such record (a folded variant the
    /// table does not fold here, an op of a shared arm) is recorded with no
    /// receiver.
    pub(crate) fn member_op_class(&mut self, module: u32, e: &Expr, since: usize) {
        let op = class(e);
        if matches!(op, Op::Folded | Op::Unstated) {
            let own = (self.member_ops.len() > since)
                .then(|| self.member_ops.last())
                .flatten()
                .is_some_and(|o| o.recv.is_some() && o.op == crate::gen::variant_name(e));
            if !own {
                self.member_op(module, e, "", None, None, false);
            }
        } else if let Op::Static(ns, key) | Op::Construct(ns, key) | Op::Internal(ns, key) = op {
            self.member_op(module, e, key, None, Some(ns), false);
            if let Some(o) = self.member_ops.last_mut() {
                o.form = match op {
                    Op::Construct(..) => MemberForm::Construct,
                    Op::Internal(..) => MemberForm::Internal,
                    _ => MemberForm::Member,
                };
            }
        }
    }
}
