use std::fmt;

/// Canonical native-library ABI descriptor used by external
/// `perry.nativeLibrary.functions` declarations.
///
/// These descriptors describe the native boundary slots, not the
/// JavaScript-visible type system. For example, [`NativeAbiType::F32`] is a
/// native ABI slot and materializes to a JavaScript `number` through an
/// explicit `f32 -> f64` transition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NativeAbiType {
    /// NaN-boxed Perry JavaScript value (`double` in LLVM IR).
    JsValue,
    /// JavaScript string passed/returned through a raw runtime string pointer.
    String,
    /// JavaScript truthiness lowered to a C `i32` boolean slot.
    Bool,
    /// Signed 32-bit integer slot.
    I32,
    /// Signed 64-bit integer slot.
    I64,
    /// Legacy string return where the native function returns the string
    /// pointer as an `i64` instead of a C pointer.
    I64String,
    /// Unsigned 32-bit integer slot.
    U32,
    /// Unsigned 64-bit integer slot.
    U64,
    /// Pointer-sized unsigned integer slot. Perry's native runtime targets are
    /// currently 64-bit, so this lowers as an LLVM `i64`.
    USize,
    /// 32-bit float slot.
    F32,
    /// 64-bit float slot. The legacy manifest spelling `"number"` is accepted
    /// as an alias and canonicalizes to this descriptor.
    F64,
    /// Raw pointer or opaque pointer-sized slot.
    Ptr,
    /// Buffer byte length slot.
    BufferLen,
    /// Native-call convenience descriptor: one JavaScript Buffer/Uint8Array
    /// argument lowers to two ABI slots, `(ptr, usize)`.
    BufferAndLen,
    /// Opaque native handle, optionally tagged with author metadata.
    Handle(Option<String>),
    /// Opaque native promise boundary handle with optional result metadata.
    Promise(Box<NativeAbiType>),
    /// No return value. This is valid only as a return descriptor.
    Void,
}

impl NativeAbiType {
    /// Parse a string descriptor from a manifest.
    pub fn parse_str(spelling: &str) -> Result<Self, NativeAbiParseError> {
        let trimmed = spelling.trim();
        let lower = trimmed.to_ascii_lowercase();
        match lower.as_str() {
            "jsvalue" | "js_value" => Ok(Self::JsValue),
            "string" => Ok(Self::String),
            "bool" | "boolean" => Ok(Self::Bool),
            "i32" => Ok(Self::I32),
            "i64" => Ok(Self::I64),
            "i64_str" => Ok(Self::I64String),
            "u32" => Ok(Self::U32),
            "u64" => Ok(Self::U64),
            "usize" => Ok(Self::USize),
            "f32" => Ok(Self::F32),
            "f64" | "number" => Ok(Self::F64),
            "ptr" => Ok(Self::Ptr),
            "buffer_len" => Ok(Self::BufferLen),
            "buffer+len" => Ok(Self::BufferAndLen),
            "handle" => Ok(Self::Handle(None)),
            "promise" => Ok(Self::Promise(Box::new(Self::JsValue))),
            "void" => Ok(Self::Void),
            _ => {
                if let Some(inner) = trimmed
                    .strip_prefix("handle<")
                    .and_then(|s| s.strip_suffix('>'))
                {
                    let handle_type = inner.trim();
                    if handle_type.is_empty() {
                        return Err(NativeAbiParseError::new(
                            trimmed,
                            "handle<T> requires a non-empty T",
                        ));
                    }
                    return Ok(Self::Handle(Some(handle_type.to_string())));
                }
                if let Some(inner) = trimmed
                    .strip_prefix("promise<")
                    .and_then(|s| s.strip_suffix('>'))
                {
                    let result = inner.trim();
                    if result.is_empty() {
                        return Err(NativeAbiParseError::new(
                            trimmed,
                            "promise<T> requires a non-empty T",
                        ));
                    }
                    return Ok(Self::Promise(Box::new(Self::parse_str(result)?)));
                }
                Err(NativeAbiParseError::new(trimmed, "unknown native ABI type"))
            }
        }
    }

    /// Canonical descriptor kind, excluding metadata.
    pub fn canonical_kind(&self) -> &'static str {
        match self {
            Self::JsValue => "jsvalue",
            Self::String => "string",
            Self::Bool => "bool",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::I64String => "i64_str",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::USize => "usize",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Ptr => "ptr",
            Self::BufferLen => "buffer_len",
            Self::BufferAndLen => "buffer+len",
            Self::Handle(_) => "handle",
            Self::Promise(_) => "promise",
            Self::Void => "void",
        }
    }

    /// Number of native ABI slots consumed or produced by this descriptor.
    pub fn abi_slot_count(&self) -> usize {
        match self {
            Self::Void => 0,
            Self::BufferAndLen => 2,
            _ => 1,
        }
    }

    /// Return the optional handle metadata attached to `handle<T>`.
    pub fn handle_type(&self) -> Option<&str> {
        match self {
            Self::Handle(Some(ty)) => Some(ty.as_str()),
            _ => None,
        }
    }

    /// Return the optional promise result metadata attached to `promise<T>`.
    pub fn promise_result(&self) -> Option<&NativeAbiType> {
        match self {
            Self::Promise(result) => Some(result.as_ref()),
            _ => None,
        }
    }

    /// True when this descriptor is legal in a parameter list.
    pub fn is_valid_param(&self) -> bool {
        !matches!(self, Self::Void)
    }

    /// True when this descriptor is legal as a return type.
    pub fn is_valid_return(&self) -> bool {
        !matches!(self, Self::BufferAndLen)
    }

    /// Render the JavaScript-facing type used in generated docs and `.d.ts`
    /// surfaces.
    pub fn js_type_name(&self) -> &'static str {
        match self {
            Self::String | Self::I64String => "string",
            Self::Bool => "boolean",
            Self::Void => "void",
            Self::Promise(_) => "Promise<any>",
            Self::Handle(_) | Self::Ptr | Self::JsValue => "any",
            Self::BufferAndLen => "Buffer",
            Self::I32
            | Self::I64
            | Self::U32
            | Self::U64
            | Self::USize
            | Self::F32
            | Self::F64
            | Self::BufferLen => "number",
        }
    }
}

impl fmt::Display for NativeAbiType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Handle(Some(ty)) => write!(f, "handle<{ty}>"),
            Self::Handle(None) => f.write_str("handle"),
            Self::Promise(result) => write!(f, "promise<{result}>"),
            other => f.write_str(other.canonical_kind()),
        }
    }
}

/// Error returned when a manifest descriptor spelling is not part of the
/// native ABI vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAbiParseError {
    spelling: String,
    reason: &'static str,
}

impl NativeAbiParseError {
    fn new(spelling: impl Into<String>, reason: &'static str) -> Self {
        Self {
            spelling: spelling.into(),
            reason,
        }
    }

    /// The descriptor spelling that failed to parse.
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    /// Human-readable parse failure reason.
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for NativeAbiParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid native ABI type {:?}: {}",
            self.spelling, self.reason
        )
    }
}

impl std::error::Error for NativeAbiParseError {}
