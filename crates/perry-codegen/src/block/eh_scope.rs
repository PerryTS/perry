//! Owns one entry of the function's EH-scope stack: the landing pad a scope unwinds to and the pad an inner action scope continues at.

/// One entry of the function's EH-scope stack (ehfree design §4.1).
///
/// `pad` is the unwind destination every call emitted inside the scope
/// targets (empty on WASI, whose `try` uses setjmp transport and has no
/// landing pad). `after_pad` is the ordinary block an INNER action scope
/// branches to once its own actions ran: the catch entry for a handler
/// scope, or this scope's action block for an action scope. `None` when the
/// scope cannot be chained to (WASI, or a scope pushed by a unit test).
///
/// `handle_height` is the `i64` SSA value holding the runtime-handle stack's
/// height when the handler scope was entered (`js_eh_handle_height`; an
/// action scope carries its handler's); every pad of the scope passes it to
/// `js_eh_pad_enter`. `None` on WASI and in unit tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EhScope {
    pub pad: String,
    pub after_pad: Option<String>,
    /// True for a scope whose pad runs actions of the same function and then
    /// continues at the enclosing scope's `after_pad` (`stmt::eh_actions`);
    /// false for a real handler (`try`, the async rejection boundary).
    pub is_action: bool,
    pub handle_height: Option<String>,
}

impl EhScope {
    /// A handler scope whose pad funnels into `after_pad`.
    pub fn handler(pad: String, after_pad: Option<String>) -> Self {
        Self {
            pad,
            after_pad,
            is_action: false,
            handle_height: None,
        }
    }
}
