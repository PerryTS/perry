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
//! * **Runtime recording** on Linux uses scripts/record_function_order.py:
//!   perf samples a symbol-preserving representative executable and emits
//!   raw ELF symbols for hot generated, runtime and stdlib functions.
//! * **Consuming** places listed functions together, in list order. Linux
//!   uses a linker script to collect generated ranked sections and runtime
//!   archive function sections into a 2 MiB-aligned prefix. Its linker-defined
//!   bounds limit startup huge-page backing to complete 2 MiB pages of selected
//!   code. Smaller prefixes only change layout and incur no backing work. Other ELF links use symbol ordering and Mach-O uses order_file.
//!   Unlisted functions retain their optimization and remain available.
//!
//! Without an order file neither startup IR nor link arguments change.
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

/// Prefix of the ranked sections of the JS text section (x86_64 Linux puts
/// every generated function in `perry_js_text`): the JS hot run, gathered in
/// rank order at the front of that output section.
pub const JS_SORTED_SECTION_PREFIX: &str = "perry_js_text.sorted.";

/// Prefix of a JS body's own input section (`split_js_text`): the link
/// drops each one nothing references and gathers the rest into the JS text
/// output section ([`JS_TEXT_LINK_RULE`]).
pub const JS_FN_SECTION_PREFIX: &str = "perry_js_text.fn.";

/// The ELF link rule that gathers the JS text input sections into the
/// `perry_js_text` output section: a C-identifier name, so the linker defines
/// `__start_`/`__stop_perry_js_text` around every JS frame.
pub const JS_TEXT_LINK_RULE: &str =
    "SECTIONS {\n perry_js_text : ALIGN(16) {\n  *(perry_js_text perry_js_text.fn.*)\n }\n} INSERT AFTER .text;\n";

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

    /// The section a listed function of the JS text section is placed in:
    /// its rank inside that section, so the ranked JS functions form the
    /// JS hot run at its front (`elf_linker_script`) and every JS frame
    /// stays inside the range the `Error` capture classifies by.
    pub fn js_section_for(&self, name: &str) -> Option<String> {
        self.rank(name)
            .map(|rank| format!("{JS_SORTED_SECTION_PREFIX}{rank:08}"))
    }

    /// An ELF linker-script augmentation: consume the selected generated
    /// sections and runtime archive sections before the default .text rule.
    /// Rust uses function sections by default on Linux; no archive rewriting
    /// or code duplication is needed. Only complete pages of selected code are
    /// eligible: no padding is inserted between selected and unlisted text.
    pub fn elf_linker_script(&self) -> Result<String, &'static str> {
        let mut out =
            String::from("SECTIONS {\n .text : ALIGN(16) {\n  __perry_hot_text_start = .;\n");
        for (rank, name) in self.names.iter().enumerate() {
            // Names enter a linker pattern. Reject metacharacters rather than
            // allowing an order file to select unrelated sections or commands.
            if !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.$".contains(&b))
            {
                return Err("function order contains an unsafe ELF symbol name");
            }
            out.push_str(&format!(
                "  *(.text.sorted.{rank:08} .text.{name} .text.hot.{name} .text.unlikely.{name} .text.startup.{name} .text.exit.{name} .gnu.linkonce.t.{name})\n"
            ));
        }
        out.push_str(
            "  __perry_hot_text_end = .;\n  __perry_hot_text_limit = __perry_hot_text_end;\n  *(.text.unlikely .text.*_unlikely .text.unlikely.*)\n  *(.text.exit .text.exit.*)\n  *(.text.startup .text.startup.*)\n  *(.text.hot .text.hot.*)\n  *(SORT(.text.sorted.*))\n  *(.text .stub .text.* .gnu.linkonce.t.*)\n }\n} INSERT BEFORE .text;\n",
        );
        // The JS text section: its ranked functions first (the JS hot run,
        // backed like the runtime's), then the rest. A C-identifier output
        // section name, so the linker defines `__start_`/`__stop_perry_js_text`
        // around both: every JS frame is in that range.
        out.push_str(
            "SECTIONS {\n perry_js_text : ALIGN(16) {\n  __perry_hot_js_text_start = .;\n  *(SORT(perry_js_text.sorted.*))\n  __perry_hot_js_text_end = .;\n  __perry_hot_js_text_limit = __perry_hot_js_text_end;\n  *(perry_js_text perry_js_text.fn.*)\n }\n} INSERT AFTER .text;\n",
        );
        Ok(out)
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
            FunctionLayout::Order(order) => format!("order-v4:{:016x}", order.digest()),
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

    /// A ranked JS body takes its rank inside the JS text section, and the
    /// script gathers those ranks at the front of that output section with
    /// their own hot-run bounds: every JS frame stays in the range the
    /// `Error` capture classifies by, and the JS hot run is backed as the
    /// runtime's is.
    #[test]
    fn elf_order_keeps_the_js_hot_run_inside_the_js_text_section() {
        let order = FunctionOrder::parse("perry_fn_a\njs_string_concat\n");
        assert_eq!(order.js_section_for("perry_fn_a").as_deref(), Some("perry_js_text.sorted.00000000"));
        assert_eq!(order.section_for("js_string_concat").as_deref(), Some(".text.sorted.00000001"));
        let script = order.elf_linker_script().unwrap();
        let js = script.split("perry_js_text : ALIGN(16)").nth(1).expect("JS text output section");
        let first = js.find("*(SORT(perry_js_text.sorted.*))").expect("ranked first");
        let rest = js.find("*(perry_js_text perry_js_text.fn.*)").expect("then the rest");
        assert!(first < rest);
        assert!(js.contains("__perry_hot_js_text_start"));
        assert!(js.contains("__perry_hot_js_text_limit = __perry_hot_js_text_end"));
        assert!(script.contains("INSERT AFTER .text"));
    }

    #[test]
    fn elf_order_selects_runtime_and_generated_sections_and_rejects_patterns() {
        let order = FunctionOrder::parse("js_string_concat\n_ZN13perry_runtime3foo17h123E\nmain\n");
        let script = order.elf_linker_script().unwrap();
        assert!(script.contains(".text.sorted.00000000 .text.js_string_concat"));
        assert!(script.contains(".text._ZN13perry_runtime3foo17h123E"));
        assert!(script.contains("__perry_hot_text_limit"));
        assert!(script.contains(".text : ALIGN(16)"));
        assert!(script.contains("__perry_hot_text_limit = __perry_hot_text_end"));
        assert!(!script.contains(". = ALIGN"), "no padding inside text");
        for bad in ["*", "a?b", "a[b]", "a\"b", "a;evil"] {
            assert!(
                FunctionOrder::parse(bad).elf_linker_script().is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn linker_files_use_each_formats_symbol_names() {
        let order = FunctionOrder::parse("main\nperry_fn_a\n");
        assert_eq!(order.linker_ordering_file(false), "main\nperry_fn_a\n");
        assert_eq!(order.linker_ordering_file(true), "_main\n_perry_fn_a\n");
    }
}
