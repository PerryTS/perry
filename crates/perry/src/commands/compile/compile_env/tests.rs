use super::{COMPILE_ENV_EXCLUSIONS, COMPILE_ENV_VARS};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// A small lexical scanner: skip comments and treat strings as single tokens,
// so examples in documentation and embedded Rust/TypeScript are not env reads.
// Keep punctuation separate to accept spaces/newlines in paths and macro calls.
fn tokens(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if bytes[i] == b'\'' && bytes.get(i + 1) == Some(&b'\\') {
            // Escaped character literals (including b'\"') must not start a string.
            let mut end = i + 3;
            while end < bytes.len() && bytes[end] != b'\'' && end - i < 16 {
                end += 1;
            }
            if bytes.get(end) == Some(&b'\'') {
                i = end + 1;
            } else {
                out.push("'".to_owned());
                i += 1;
            }
        } else if bytes[i] == b'\'' && i + 1 < bytes.len() {
            let end = i + 1 + source[i + 1..].chars().next().unwrap().len_utf8();
            if bytes.get(end) == Some(&b'\'') {
                i = end + 1;
            } else {
                // A lifetime, such as 'static, is punctuation plus a word.
                out.push("'".to_owned());
                i += 1;
            }
        } else {
            let start = i;
            // Raw strings, including arbitrary hash delimiters.
            let mut quote = i;
            if bytes[i] == b'r' {
                quote += 1;
                while quote < bytes.len() && bytes[quote] == b'#' {
                    quote += 1;
                }
            }
            if bytes[i] == b'r' && bytes.get(quote) == Some(&b'"') {
                let hashes = quote - start - 1;
                i = quote + 1;
                let content = i;
                while i < bytes.len() {
                    if bytes[i] == b'"'
                        && bytes.get(i + 1..i + 1 + hashes) == Some(&bytes[start + 1..quote])
                    {
                        break;
                    }
                    i += 1;
                }
                out.push(format!("\"{}\"", &source[content..i]));
                i = (i + 1 + hashes).min(bytes.len());
            } else if bytes[i] == b'"' {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i = (i + 2).min(bytes.len());
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                out.push(source[start..i].to_owned());
            } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(source[start..i].to_owned());
            } else {
                // Non-ASCII punctuation is irrelevant to the ASCII env names.
                let ch = source[i..].chars().next().unwrap();
                out.push(ch.to_string());
                i += ch.len_utf8();
            }
        }
    }
    out
}

fn env_reads(source: &str) -> Vec<String> {
    let tokens = tokens(source);
    let mut found = Vec::new();
    // Resolve simple named string constants used as env-read arguments too.
    let mut constants = BTreeMap::new();
    for (i, token) in tokens.iter().enumerate() {
        if token != "const" {
            continue;
        }
        let Some(name) = tokens.get(i + 1) else {
            continue;
        };
        let mut j = i + 2;
        while j < tokens.len() && !matches!(tokens[j].as_str(), "=" | ";" | "{") {
            j += 1;
        }
        if tokens.get(j).map(String::as_str) == Some("=") {
            if let Some(value) = tokens.get(j + 1).filter(|v| v.starts_with('"')) {
                constants.insert(name.as_str(), value.as_str());
            }
        }
    }
    for (i, token) in tokens.iter().enumerate() {
        let mut j = i + 1;
        match token.as_str() {
            "var" | "var_os" | "env_flag" => {}
            "env" | "option_env" if tokens.get(j).map(String::as_str) == Some("!") => {
                j += 1;
            }
            _ => continue,
        }
        if !matches!(tokens.get(j).map(String::as_str), Some("(" | "{" | "[")) {
            continue;
        }
        let Some(name) = tokens
            .get(j + 1)
            .map(|s| constants.get(s.as_str()).copied().unwrap_or(s.as_str()))
            .and_then(|s| s.strip_prefix('"'))
            .and_then(|s| s.strip_suffix('"'))
        else {
            continue;
        };
        if name.starts_with("PERRY_")
            && name.len() > 6
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            found.push(name.to_owned());
        }
    }
    found
}

// Discover every workspace dependency the compiler can link, including optional,
// target and build dependencies. A newly linked crate joins the audit automatically.
fn compiler_crates(crates: &Path) -> Vec<PathBuf> {
    let mut manifests = BTreeMap::new();
    for entry in std::fs::read_dir(crates).expect("workspace crates") {
        let dir = entry.unwrap().path();
        let path = dir.join("Cargo.toml");
        if !path.is_file() {
            continue;
        }
        let manifest = toml::Value::Table(
            std::fs::read_to_string(path)
                .unwrap()
                .parse::<toml::Table>()
                .unwrap(),
        );
        let name = manifest["package"]["name"].as_str().unwrap().to_owned();
        manifests.insert(name, (dir, manifest));
    }
    let mut todo = vec!["perry".to_owned()];
    let mut seen = BTreeSet::new();
    let mut roots = Vec::new();
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let (dir, manifest) = &manifests[&name];
        roots.push(dir.clone());
        let mut tables = vec![manifest];
        if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
            tables.extend(targets.values());
        }
        for table in tables {
            for kind in ["dependencies", "build-dependencies"] {
                if let Some(deps) = table.get(kind).and_then(toml::Value::as_table) {
                    for (alias, dep) in deps {
                        let name = dep
                            .get("package")
                            .and_then(toml::Value::as_str)
                            .unwrap_or(alias);
                        if manifests.contains_key(name) {
                            todo.push(name.to_owned());
                        }
                    }
                }
            }
        }
    }
    roots
}

#[test]
fn env_read_scanner_handles_all_read_forms_without_scanning_examples() {
    let source = r####"
        std::env :: var (
            "PERRY_ONE");
        env::var_os( r#"PERRY_TWO"# );
        option_env! ("PERRY_THREE"); env!("PERRY_FOUR");
        var("PERRY_FIVE"); env_flag("PERRY_SIX");
        const NAMED: &str = "PERRY_SEVEN"; env::var_os(NAMED);
        env! { "PERRY_EIGHT" }; option_env!["PERRY_NINE"];
        // env::var("PERRY_COMMENT")
        /* /* nested */ env::var_os("PERRY_COMMENT") */
        let example = r###"env::var("PERRY_EXAMPLE")"###;
        let ordinary = "env::var(\"PERRY_EXAMPLE\")";
        let quote = '"'; let escaped = '\''; let byte_quote = b'"';
        env::var("OTHER_NAME"); env::var("PERRY_TEN");
    "####;
    assert_eq!(
        env_reads(source),
        [
            "PERRY_ONE",
            "PERRY_TWO",
            "PERRY_THREE",
            "PERRY_FOUR",
            "PERRY_FIVE",
            "PERRY_SIX",
            "PERRY_SEVEN",
            "PERRY_EIGHT",
            "PERRY_NINE",
            "PERRY_TEN"
        ]
    );
}

#[test]
fn codegen_env_vars_are_build_cache_inputs() {
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let roots = compiler_crates(&crates);
    assert!(
        roots.len() >= 24,
        "compiler dependency scan shrank unexpectedly"
    );
    let mut found = BTreeSet::new();
    let mut reads = 0;
    for root in roots {
        let mut stack = vec![root.join("src"), root.join("build.rs")];
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                stack.extend(std::fs::read_dir(path).unwrap().map(|e| e.unwrap().path()));
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") && path.is_file() {
                let names = env_reads(&std::fs::read_to_string(&path).unwrap());
                reads += names.len();
                found.extend(names);
            }
        }
    }
    // Experiments and compiler dependencies can be removed legitimately.
    // Check representative live reads (including the new diagnostic reader)
    // rather than freezing the historical inventory's cardinality. The root
    // count above and the scanner-form test still prevent a vacuous audit.
    for name in [
        "PERRY_ALLOW_EVAL",
        "PERRY_FACTS_MAX_WORK",
        "PERRY_FALLBACK_DIAG",
        "PERRY_LLVM_CLANG",
    ] {
        assert!(
            found.contains(name),
            "env audit missed live compiler read {name}"
        );
    }
    assert!(
        COMPILE_ENV_VARS.windows(2).all(|pair| pair[0] < pair[1]),
        "shared list must be sorted and unique"
    );
    let exclusions: BTreeSet<_> = COMPILE_ENV_EXCLUSIONS
        .iter()
        .map(|(name, reason)| {
            assert!(
                !reason.trim().is_empty() && !reason.contains('\n'),
                "missing one-line reason for {name}"
            );
            assert!(
                !COMPILE_ENV_VARS.contains(name),
                "{name} is both keyed and excluded"
            );
            assert!(found.contains(*name), "stale exclusion: {name}");
            *name
        })
        .collect();
    assert_eq!(
        exclusions.len(),
        COMPILE_ENV_EXCLUSIONS.len(),
        "duplicate exclusion"
    );
    let missing: Vec<_> = found
        .iter()
        .filter(|name| {
            !COMPILE_ENV_VARS.contains(&name.as_str()) && !exclusions.contains(name.as_str())
        })
        .collect();
    assert!(missing.is_empty(), "unregistered compiler env reads: {missing:?}; add to compile_env::COMPILE_ENV_VARS or COMPILE_ENV_EXCLUSIONS with a reason (#12582)");
    eprintln!("compile env audit: {reads} reads, {} names", found.len());
}

/// The element-rep facts' switches change emitted code, so each one keys
/// both caches (DESIGN-p4l1 §5.2): `=0` must never reuse an `=1` object.
#[test]
fn array_fact_switches_are_cache_inputs() {
    for name in [
        "PERRY_FACTS_ARRAY_BASE",
        "PERRY_FACTS_FN_BASES",
        "PERRY_FACTS_WALK_INDEXED",
        "PERRY_FACTS_ARRAY_FIELDS",
        "PERRY_FACTS_ARRAY_FLOOR",
        "PERRY_FACTS_ARRAY_LENGTHS",
        "PERRY_FACTS_ARRAY_REPS",
        "PERRY_FACTS_ARRAY_TIER",
        "PERRY_FACTS_CONST_ARRAYS",
        "PERRY_FACTS_SABOTAGE",
    ] {
        assert!(
            COMPILE_ENV_VARS.contains(&name),
            "{name} must key the compile caches"
        );
    }
}

/// N6: the module-const switch changes the facts' intervals and the reads
/// codegen emits, so `=0` must never reuse an `=1` object.
#[test]
fn const_number_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_FACTS_CONST_NUMBERS"),
        "PERRY_FACTS_CONST_NUMBERS must key the compile caches"
    );
}

/// R1: the range-tightening switches change the facts' intervals (and with
/// them in-floor reads and int32 locals), so `=0` must never reuse an `=1`
/// object.
#[test]
fn range_tightening_switches_are_cache_inputs() {
    for name in ["PERRY_FACTS_EXACT_RULES", "PERRY_FACTS_LEAVING_WRITES"] {
        assert!(
            COMPILE_ENV_VARS.contains(&name),
            "{name} must key the compile caches"
        );
    }
}

/// N10: the unsigned-bound switch changes the element reads' and stores'
/// key admission (the sign test in front of the bound), so `=0` must never
/// reuse an `=1` object.
#[test]
fn unsigned_bound_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_UNSIGNED_BOUND"),
        "PERRY_UNSIGNED_BOUND must key the compile caches"
    );
}

/// Key signs: the switch changes the exported facts and every element site
/// that consumes its key's interval, so `=0` must never reuse an `=1` object.
#[test]
fn key_sign_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_FACTS_KEY_SIGN"),
        "PERRY_FACTS_KEY_SIGN must key the compile caches"
    );
}

/// Literal strings: the switch changes the exported facts and every store
/// and alias that drops its string demote, so `=0` must never reuse an `=1`
/// object.
#[test]
fn literal_strings_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_FACTS_LITERAL_STRINGS"),
        "PERRY_FACTS_LITERAL_STRINGS must key the compile caches"
    );
}

/// The list birth capacity: the switch (and its slot count) changes the
/// exported facts and every `let x = []` birth that reserves them, so `=0`
/// must never reuse an object built with a capacity.
#[test]
fn list_birth_capacity_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_LIST_BIRTH_CAPACITY"),
        "PERRY_LIST_BIRTH_CAPACITY must key the compile caches"
    );
}

/// Bounded loops: the switch decides which loops keep their back-edge polls,
/// so `=0` must never reuse an `=1` object; the probe line is keyed with it.
#[test]
fn bounded_loops_switch_is_a_cache_input() {
    for name in ["PERRY_FACTS_BOUNDED_LOOPS", "PERRY_LOOP_POLL_DIAG"] {
        assert!(
            COMPILE_ENV_VARS.contains(&name),
            "{name} must key the compile caches"
        );
    }
}

/// Wide keys: the switch changes the exported facts and every element site
/// that converts a wide key without the normalizer, so `=0` must never reuse
/// an `=1` object.
#[test]
fn wide_keys_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_FACTS_WIDE_KEYS"),
        "PERRY_FACTS_WIDE_KEYS must key the compile caches"
    );
}

/// The number-over-number store arm: the switch changes every guarded element
/// store of a Number, so `=0` must never reuse an `=1` object.
#[test]
fn number_store_arm_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_NUMBER_STORE_ARM"),
        "PERRY_NUMBER_STORE_ARM must key the compile caches"
    );
}

/// The rep-keyed element store: the switch changes every runtime-key store
/// into an Array the facts give Number elements, so `=0` must never reuse an
/// `=1` object.
#[test]
fn array_store_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_FACTS_ARRAY_STORE"),
        "PERRY_FACTS_ARRAY_STORE must key the compile caches"
    );
}

/// Plain `slice` births: the switch changes the exported array reps, so `=0`
/// must never reuse an `=1` object.
#[test]
fn slice_births_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_FACTS_SLICE_BIRTHS"),
        "PERRY_FACTS_SLICE_BIRTHS must key the compile caches"
    );
}

/// `a[k] ?? <literal>` over an Array of Numbers: the switch changes the
/// read's lowering, so `=0` must never reuse an `=1` object.
#[test]
fn rep_default_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_REP_DEFAULT"),
        "PERRY_REP_DEFAULT must key the compile caches"
    );
}

/// Typed `set`: the signed-index switch decides which methods get a typed
/// body in place of their index clones, so `=0` must never reuse an `=1`
/// object.
#[test]
fn signed_index_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_SIG_SIGNED_INDEX"),
        "PERRY_SIG_SIGNED_INDEX must key the compile caches"
    );
}

/// The local-range switch decides whether codegen's range proofs read the
/// facts' interval of an int32 local, so `=0` must never reuse an `=1`
/// object.
#[test]
fn local_ranges_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_FACTS_LOCAL_RANGES"),
        "PERRY_FACTS_LOCAL_RANGES must key the compile caches"
    );
}

/// The int32-operand switches decide whether a bitwise operator over an
/// I32 lane or an int32 element read is lowered in i32, so `=0` must never
/// reuse an `=1` object.
#[test]
fn int32_operand_switches_are_cache_inputs() {
    for name in [
        "PERRY_FACTS_INT32_READS",
        "PERRY_INT32_OPERANDS",
        "PERRY_INT32_SHADOWS",
    ] {
        assert!(
            COMPILE_ENV_VARS.contains(&name),
            "{name} must key the compile caches"
        );
    }
}

/// The direct Number call switch decides whether a consumer of `f(a)` takes
/// the proven-Number lowering, so `=0` must never reuse an `=1` object.
#[test]
fn direct_number_calls_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_FACTS_DIRECT_NUMBER_CALLS"),
        "PERRY_FACTS_DIRECT_NUMBER_CALLS must key the compile caches"
    );
}

/// The at-rest default read's switch decides how `a[k] ?? <int32>` is
/// lowered, so `=0` must never reuse an `=1` object.
#[test]
fn rest_default_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_REST_DEFAULT"),
        "PERRY_REST_DEFAULT must key the compile caches"
    );
}

/// N4: the cold-path switch changes helper declarations, call-site
/// attributes and branch weights, so `=0` must never reuse an `=1` object.
#[test]
fn cold_path_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_COLD_PATHS"),
        "PERRY_COLD_PATHS must key the compile caches"
    );
}

/// N12: the switch decides which stack checks a body keeps, so `=0` must
/// never reuse an object built with it on.
#[test]
fn prologue_cost_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_PROLOGUE_COST"),
        "PERRY_PROLOGUE_COST must key the compile caches"
    );
}

/// Value records (DESIGN-p4l4 §7): the switch changes the facts (and, from
/// VR-3, emitted code), so `=1` must never reuse an object built without it.
#[test]
fn value_records_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_FACTS_VALUE_RECORDS"),
        "PERRY_FACTS_VALUE_RECORDS must key the compile caches"
    );
}

/// The pure-argument switch decides whether the inliner substitutes a
/// written local for a parameter (and so whether a call in a loop condition
/// is inlined at all), so `=0` must never reuse an `=1` object.
#[test]
fn inline_pure_args_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_INLINE_PURE_ARGS"),
        "PERRY_INLINE_PURE_ARGS must key the compile caches"
    );
}

/// The small-string switch decides how a string `+`, a one-digit
/// `String(n)` and a `charAt` operand are lowered, so `=0` must never reuse
/// an `=1` object.
#[test]
fn small_strings_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_SMALL_STRINGS"),
        "PERRY_SMALL_STRINGS must key the compile caches"
    );
}

/// The call-kinds switch decides whether the facts export a call's result
/// kinds, and so how a `+` of two calls is lowered: `=0` must never reuse an
/// `=1` object.
#[test]
fn call_kinds_switch_is_a_cache_input() {
    assert!(
        COMPILE_ENV_VARS.contains(&"PERRY_FACTS_CALL_KINDS"),
        "PERRY_FACTS_CALL_KINDS must key the compile caches"
    );
}
