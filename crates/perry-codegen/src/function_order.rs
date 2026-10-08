//! Function layout from a recorded first-execution order.
//!
//! A generated function's text is first touched when it first runs. Units are
//! laid out in partition order, which has nothing to do with when code runs,
//! so a large program's startup touches functions spread over its whole text,
//! and each first touch maps a whole page-cache folio of unrelated neighbours.
//! Static reachability cannot fix this (most of what runs at startup is
//! reached through closures and method dispatch), so the order comes from one
//! real run:
//!
//! * **Recording** is a separate build (`perry compile --record-function-order`).
//!   Every function defined in the final, optimized module calls
//!   `js_function_order_first_call` on entry with a private per-function
//!   record `{flag byte, name}`; the runtime appends the name to
//!   `$PERRY_FUNCTION_ORDER_OUT` the first time the flag flips. A normal build
//!   never references the hook, so it costs nothing when unused.
//! * **Consuming** (`perry compile --function-order FILE`) places the listed
//!   functions first, in list order: on ELF each listed function gets its own
//!   `.text.sorted.<rank>` section, which GNU ld's default script sorts by
//!   name at the head of `.text` (lld links also get `--symbol-ordering-file`);
//!   Mach-O links get ld64's `-order_file`. Unlisted functions keep today's
//!   order. Only functions the program defines are looked up in the list, so
//!   a stale or partial list is harmless.
//!
//! The list is plain text: one LLVM function name per line (the ELF symbol;
//! Mach-O symbols add a leading `_`). Blank lines and `#` comments are
//! ignored, and the first occurrence of a name decides its rank.
//!
//! Both the instrumentation and the section assignment run on the final LLVM
//! module just before emission (`inprocess::function_layout`), so the
//! recorded names are exactly the symbols a normal build of the same program
//! emits, whatever the optimizer inlined.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// The runtime entry a recording build calls at the top of every function.
pub const RECORD_HOOK: &str = "js_function_order_first_call";

/// The environment variable a recording binary reads for its output path.
pub const RECORD_OUTPUT_ENV: &str = "PERRY_FUNCTION_ORDER_OUT";

/// Prefix of the per-function ELF sections that carry a rank. GNU ld's
/// default linker script gathers `SORT(.text.sorted.*)` ahead of the rest of
/// `.text`, so the zero-padded rank orders them.
pub const SORTED_SECTION_PREFIX: &str = ".text.sorted.";

/// A parsed order list: function name -> rank (0 runs first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionOrder {
    names: Vec<String>,
    ranks: HashMap<String, u32>,
}

impl FunctionOrder {
    /// Parse a list. Never fails: anything that is not a name is skipped,
    /// and a repeated name keeps its first rank.
    pub fn parse(text: &str) -> Self {
        let mut names = Vec::new();
        let mut ranks = HashMap::new();
        for line in text.lines() {
            let name = line.trim();
            if name.is_empty() || name.starts_with('#') || name.contains(char::is_whitespace) {
                continue;
            }
            if ranks.contains_key(name) {
                continue;
            }
            ranks.insert(name.to_string(), names.len() as u32);
            names.push(name.to_string());
        }
        Self { names, ranks }
    }

    /// The rank of `name`, or `None` for a function the list does not name.
    pub fn rank(&self, name: &str) -> Option<u32> {
        self.ranks.get(name).copied()
    }

    /// The names in rank order.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The ELF section a listed function is placed in.
    pub fn section_for(&self, name: &str) -> Option<String> {
        self.rank(name)
            .map(|rank| format!("{SORTED_SECTION_PREFIX}{rank:08}"))
    }

    /// A content digest for the object-cache key: two lists that rank the
    /// same names the same way share it, whatever their comments say.
    pub fn digest(&self) -> u64 {
        // FNV-1a: stable across processes and toolchains, unlike `DefaultHasher`.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for name in &self.names {
            for b in name.bytes().chain(std::iter::once(b'\n')) {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    /// The list as a linker ordering file: lld's `--symbol-ordering-file`
    /// takes the names as they are; ld64's `-order_file` takes Mach-O symbol
    /// names, which carry a leading underscore.
    pub fn linker_ordering_file(&self, mach_o: bool) -> String {
        let mut out = String::new();
        for name in &self.names {
            if mach_o {
                out.push('_');
            }
            out.push_str(name);
            out.push('\n');
        }
        out
    }
}

/// What the program's function layout pass does.
#[derive(Debug, Clone, Default)]
pub enum FunctionLayout {
    /// Today's layout; no instrumentation.
    #[default]
    Default,
    /// Instrument every function to record its first execution.
    Record,
    /// Place the listed functions first, in list order.
    Order(Arc<FunctionOrder>),
}

impl FunctionLayout {
    /// The object-cache key component: every mode changes emitted objects.
    pub fn cache_key(&self) -> String {
        match self {
            FunctionLayout::Default => String::new(),
            FunctionLayout::Record => "record-v1".to_string(),
            FunctionLayout::Order(order) => format!("order-v1:{:016x}", order.digest()),
        }
    }
}

static PROGRAM_FUNCTION_LAYOUT: RwLock<FunctionLayout> = RwLock::new(FunctionLayout::Default);

/// Set by the driver before module codegen, like the program's Worker flag.
pub fn set_program_function_layout(layout: FunctionLayout) {
    *PROGRAM_FUNCTION_LAYOUT.write().unwrap() = layout;
}

/// See [`set_program_function_layout`].
pub fn program_function_layout() -> FunctionLayout {
    PROGRAM_FUNCTION_LAYOUT.read().unwrap().clone()
}

/// Object-cache key component for the program's layout.
pub fn program_function_layout_key() -> String {
    PROGRAM_FUNCTION_LAYOUT.read().unwrap().cache_key()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_noise_and_keeps_the_first_rank() {
        let order = FunctionOrder::parse(
            "# recorded by perry\n\nmain\r\n  m__init  \nperry_fn_a\nmain\nnot a name\nperry_fn_b\n",
        );
        assert_eq!(
            order.names(),
            ["main", "m__init", "perry_fn_a", "perry_fn_b"]
        );
        assert_eq!(order.rank("main"), Some(0));
        assert_eq!(order.rank("perry_fn_b"), Some(3));
        assert_eq!(order.rank("unknown"), None);
    }

    #[test]
    fn sections_sort_by_rank_as_names() {
        let names: Vec<String> = (0..12).map(|i| format!("f{i}")).collect();
        let order = FunctionOrder::parse(&names.join("\n"));
        let sections: Vec<String> = names
            .iter()
            .map(|n| order.section_for(n).unwrap())
            .collect();
        let mut sorted = sections.clone();
        sorted.sort();
        assert_eq!(
            sections, sorted,
            "a linker sorting section names must reproduce the rank"
        );
        assert_eq!(sections[10], ".text.sorted.00000010");
        assert_eq!(order.section_for("unlisted"), None);
    }

    #[test]
    fn digest_follows_ranking_not_comments() {
        let a = FunctionOrder::parse("a\nb\n");
        let b = FunctionOrder::parse("# x\na\n\nb\na\n");
        let c = FunctionOrder::parse("b\na\n");
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), c.digest());
        assert_ne!(
            FunctionLayout::Order(Arc::new(a)).cache_key(),
            FunctionLayout::Order(Arc::new(c)).cache_key()
        );
        assert_ne!(
            FunctionLayout::Record.cache_key(),
            FunctionLayout::Default.cache_key()
        );
    }

    #[test]
    fn linker_files_use_each_formats_symbol_names() {
        let order = FunctionOrder::parse("main\nperry_fn_a\n");
        assert_eq!(order.linker_ordering_file(false), "main\nperry_fn_a\n");
        assert_eq!(order.linker_ordering_file(true), "_main\n_perry_fn_a\n");
    }
}
