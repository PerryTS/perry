//! Build-only association between a body's owning fn_info! static and realm
//! declarations of that body. Nothing here is linked into perry-runtime.
use std::collections::{BTreeMap, BTreeSet};

use crate::realm_json::Value;
use crate::realm_source::{image_info_symbol, rust_tokens as tokens, Token};
use perry_abi::JsFunctionInfo;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct InfoKey {
    body: String,
    // The physical ABI facts, in the same order as FunctionInfoFacts. Public
    // name and length in a function's property bag are not its body's ABI.
    facts: [u64; 6],
}

#[derive(Default)]
pub(super) struct InfoExports(BTreeMap<InfoKey, (String, String)>);

fn qualified(body: &str, package: &str) -> String {
    body.strip_prefix("crate::").map_or_else(
        || body.into(),
        |tail| format!("{}::{tail}", package.replace('-', "_")),
    )
}

fn bare_body(declaration: &Value) -> Option<String> {
    declaration.get("function_info")?;
    let body = declaration.get("body")?.string()?;
    (!body.contains("::")).then(|| body.to_owned())
}

fn declaration_body(declaration: &Value) -> Option<String> {
    let info = declaration.get("function_info")?;
    let body = declaration.get("body")?.string()?;
    let source = declaration
        .get("source")
        .and_then(Value::string)
        .unwrap_or("");
    let source_symbol = info
        .get("source_symbol")
        .and_then(Value::string)
        .unwrap_or("");
    let owner = source_symbol
        .strip_prefix("source:")
        .and_then(|s| s.split_once("::"))
        .map(|(package, _)| package)
        .filter(|p| p.starts_with("perry_"))
        .unwrap_or_else(|| source.split('/').next().unwrap_or(""));
    // Legacy declarations on a body item may spell only its local name. Its
    // source is the owning Rust module, not an installer or generated table.
    // WRT3a2 moves these facts onto the owning fn_info declaration itself.
    if !body.contains("::") {
        let module = source.split_once("/src/")?.1.strip_suffix(".rs")?;
        let module = module.strip_suffix("/mod").unwrap_or(module);
        let module = if module == "lib" {
            "".into()
        } else {
            format!("{}::", module.replace('/', "::"))
        };
        return Some(format!("{}::{module}{body}", owner.replace('-', "_")));
    }
    Some(qualified(body, owner))
}

impl InfoExports {
    pub(super) fn needed(declarations: &[Value]) -> BTreeSet<String> {
        let mut needed: BTreeSet<String> =
            declarations.iter().filter_map(declaration_body).collect();
        // A bare body is a `#[no_mangle]` link symbol: unique program-wide.
        needed.extend(declarations.iter().filter_map(bare_body));
        needed
    }

    pub(super) fn read(
        &mut self,
        source: &str,
        path: &str,
        package: &str,
        needed: &BTreeSet<String>,
    ) -> Result<(), String> {
        if !source.contains("fn_info") || !source.contains("export") {
            return Ok(());
        }
        let ts = tokens(source)?;
        for (index, token) in ts.iter().enumerate() {
            if token.text != "fn_info"
                || ts.get(index + 1).is_none_or(|t| t.text != "!")
                || ts
                    .get(index + 2)
                    .is_none_or(|t| !matches!(t.text, "(" | "{"))
            {
                continue;
            }
            let mut pos = index + 3;
            // Realm attributes nest brackets (`[Symbol.species]`); skip balanced.
            while ts.get(pos).is_some_and(|t| t.text == "#") {
                pos += 1;
                let mut depth = 0usize;
                while let Some(t) = ts.get(pos) {
                    pos += 1;
                    match t.text {
                        "[" | "(" | "{" => depth += 1,
                        "]" | ")" | "}" => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            if ts.get(pos).is_some_and(|t| t.text == "reference") {
                pos += 1;
            }
            if ts.get(pos).is_none_or(|t| t.text != "export") {
                continue;
            }
            pos += 1;
            if ts.get(pos).is_some_and(|t| t.text == "static") {
                while ts.get(pos).is_some_and(|t| t.text != "=") {
                    pos += 1;
                }
                pos += 1;
            }
            let abi = ts.get(pos).ok_or("unterminated exported fn_info")?.text;
            let native_args = abi == "native_args";
            if matches!(
                abi,
                "native_args" | "unwind" | "unwind_in_tests" | "sentinel"
            ) {
                pos += 1;
            }
            // Macro definitions forward a body token; only concrete owning
            // invocations define exports. Their expanded definitions are
            // described by the source declaration that invokes the macro.
            if ts.get(pos).is_some_and(|t| t.text == "$") {
                continue;
            }
            let body_start = pos;
            while ts.get(pos).is_some_and(|t| t.text != ",") {
                pos += 1;
            }
            let body = ts[body_start..pos]
                .iter()
                .map(|t| t.text)
                .collect::<String>();
            let leaf = body.rsplit("::").next().unwrap_or(&body);
            if !needed.contains(&qualified(&body, package)) && !needed.contains(leaf) {
                continue;
            }
            pos += 1;
            let n = scalar(
                ts.get(pos..pos + 1)
                    .ok_or("missing exported parameter count")?,
            )?;
            let n = u16::try_from(n).map_err(|_| "fn_info parameter count exceeds u16")?;
            // This is metadata evaluation only; the null code address is
            // never published or called. Use the ABI's own builders, so flag
            // effects and defaults have the same authority as runtime birth.
            let mut info = unsafe {
                JsFunctionInfo::from_code(std::ptr::null(), if native_args { 0 } else { n })
            };
            if native_args {
                info = info.with_rest_kind(n, perry_abi::FN_REST_NATIVE_ARGS);
            }
            pos += 1;
            if ts.get(pos).is_some_and(|t| t.text == ";") {
                pos += 1;
                loop {
                    let method = ts.get(pos).ok_or("unterminated info builder")?.text;
                    if matches!(method, ")" | "}") {
                        break;
                    }
                    if ts.get(pos + 1).is_none_or(|t| t.text != "(") {
                        return Err(format!("{path}: malformed info builder {method}"));
                    }
                    pos += 2;
                    let start = pos;
                    let mut depth = 1;
                    while let Some(t) = ts.get(pos) {
                        if t.text == "(" {
                            depth += 1;
                        }
                        if t.text == ")" {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        pos += 1;
                    }
                    if depth != 0 {
                        return Err(format!("{path}: unterminated info builder"));
                    }
                    let args = split(&ts[start..pos], ",");
                    let first = || scalar(args[0]);
                    info = match method {
                        "plain" => info.plain(),
                        "with_declared" => info.with_declared(
                            u16::try_from(first()?).map_err(|_| "declared overflow")?,
                        ),
                        "with_length" => info
                            .with_length(u32::try_from(first()?).map_err(|_| "length overflow")?),
                        "with_flags" => {
                            info.with_flags(u32::try_from(first()?).map_err(|_| "flags overflow")?)
                        }
                        "with_rest" => {
                            info.with_rest(u16::try_from(first()?).map_err(|_| "rest overflow")?)
                        }
                        "with_rest_kind" if args.len() == 2 => info.with_rest_kind(
                            u16::try_from(first()?).map_err(|_| "rest overflow")?,
                            u32::try_from(scalar(args[1])?).map_err(|_| "rest kind overflow")?,
                        ),
                        _ => {
                            return Err(format!(
                                "{path}: unsupported exported info builder {method}"
                            ))
                        }
                    };
                    pos += 1;
                    match ts.get(pos).map(|t| t.text) {
                        Some(",") => pos += 1,
                        Some(")" | "}") => break,
                        // `; bind binding, metadata` follows the builders.
                        Some(";") if ts.get(pos + 1).is_some_and(|t| t.text == "bind") => break,
                        _ => return Err(format!("{path}: malformed info builder list")),
                    }
                }
            }
            let key = InfoKey {
                body: qualified(&body, package),
                facts: [
                    u64::from(info.params),
                    u64::from(info.rest_fixed),
                    u64::from(info.flags),
                    u64::from(info.length),
                    u64::from(info.declared),
                    u64::from(info.plain_params().wrapping_add(1)),
                ],
            };
            let symbol = image_info_symbol(&ts, body_start, package)
                .map_err(|e| format!("{path}: {body}: {e}"))?;
            if let Some((previous, owner)) = self.0.insert(key, (symbol.clone(), path.into())) {
                if previous != symbol || owner != path {
                    return Err(format!("{path}: duplicate owning info export {symbol}, previously {previous} at {owner}"));
                }
            }
        }
        Ok(())
    }

    pub(super) fn link(&self, declarations: &mut [Value]) -> Result<(), String> {
        let mut missing = BTreeSet::new();
        for declaration in declarations {
            let Some(info) = declaration.get("function_info") else {
                continue;
            };
            let Some(body) = declaration_body(declaration) else {
                continue;
            };
            let mut facts = [0; 6];
            for (index, field) in [
                "params",
                "rest_fixed",
                "flags",
                "length",
                "declared",
                "plain_params",
            ]
            .into_iter()
            .enumerate()
            {
                let Some(Value::Number(n)) = info.get(field) else {
                    return Err(format!("missing function info {field}"));
                };
                facts[index] = n
                    .parse()
                    .map_err(|_| format!("invalid function info {field}: {n}"))?;
            }
            let key = InfoKey { body, facts };
            let by_symbol = || {
                let bare = bare_body(declaration)?;
                let suffix = format!("::{bare}");
                let mut hits = self
                    .0
                    .iter()
                    .filter(|(k, _)| k.facts == key.facts && k.body.ends_with(&suffix));
                let hit = hits.next()?;
                hits.next().is_none().then_some(hit.1)
            };
            if let Some((symbol, _)) = self.0.get(&key).or_else(by_symbol) {
                let Value::Object(declaration) = declaration else {
                    unreachable!()
                };
                declaration.insert("info_symbol".into(), Value::String(symbol.clone()));
            } else {
                missing.insert(format!("{} {:?}", key.body, key.facts));
            }
        }
        if !missing.is_empty() {
            return Err(format!(
                "realm bodies without owning fn_info exports ({}):\n{}",
                missing.len(),
                missing.into_iter().collect::<Vec<_>>().join("\n")
            ));
        }
        Ok(())
    }
}

fn split<'a, 's>(ts: &'a [Token<'s>], delimiter: &str) -> Vec<&'a [Token<'s>]> {
    let mut start = 0;
    let mut depth = 0;
    let mut parts = Vec::new();
    for (i, t) in ts.iter().enumerate() {
        match t.text {
            "(" => depth += 1,
            ")" => depth -= 1,
            _ => {}
        }
        if depth == 0 && t.text == delimiter {
            parts.push(&ts[start..i]);
            start = i + 1;
        }
    }
    parts.push(&ts[start..]);
    parts
}

fn scalar(ts: &[Token<'_>]) -> Result<u64, String> {
    if ts.first().is_some_and(|t| t.text == "(") && ts.last().is_some_and(|t| t.text == ")") {
        return scalar(&ts[1..ts.len() - 1]);
    }
    let parts = split(ts, "|");
    if parts.len() > 1 {
        return parts.into_iter().try_fold(0, |v, p| Ok(v | scalar(p)?));
    }
    let word = ts.iter().map(|t| t.text).collect::<String>();
    let constant = match word.rsplit("::").next().unwrap_or("") {
        "FN_BUILTIN" => Some(perry_abi::FN_BUILTIN),
        "FN_NON_CONSTRUCTOR" => Some(perry_abi::FN_NON_CONSTRUCTOR),
        "FN_PERMANENT_IMAGE" => Some(perry_abi::FN_PERMANENT_IMAGE),
        "FN_REST_NATIVE_ARGS" => Some(perry_abi::FN_REST_NATIVE_ARGS),
        "FN_REST_USER" => Some(perry_abi::FN_REST_USER),
        "FN_REST_SYNTHETIC_ARGUMENTS" => Some(perry_abi::FN_REST_SYNTHETIC_ARGUMENTS),
        "FN_REST_USER_AND_ARGUMENTS" => Some(perry_abi::FN_REST_USER_AND_ARGUMENTS),
        "FN_STRICT" => Some(perry_abi::FN_STRICT),
        "FN_ARROW" => Some(perry_abi::FN_ARROW),
        "FN_ASYNC" => Some(perry_abi::FN_ASYNC),
        "FN_GENERATOR" => Some(perry_abi::FN_GENERATOR),
        "FN_ASYNC_GENERATOR" => Some(perry_abi::FN_ASYNC_GENERATOR),
        "FN_RESOLVES_ARRAY_THIS" => Some(perry_abi::FN_RESOLVES_ARRAY_THIS),
        "FN_ARGUMENTS_OBJECT" => Some(perry_abi::FN_ARGUMENTS_OBJECT),
        "FN_ARGUMENTS_RESTRICTED" => Some(perry_abi::FN_ARGUMENTS_RESTRICTED),
        "FN_ARGUMENTS_MAPPED" => Some(perry_abi::FN_ARGUMENTS_MAPPED),
        "FN_REST_MASK" => Some(perry_abi::FN_REST_MASK),
        "FN_HAS_LENGTH" => Some(perry_abi::FN_HAS_LENGTH),
        "FN_HAS_DECLARED" => Some(perry_abi::FN_HAS_DECLARED),
        "FN_COMPILED_BODY" => Some(perry_abi::FN_COMPILED_BODY),
        "FN_HAS_SOURCE" => Some(perry_abi::FN_HAS_SOURCE),
        "FN_NON_STRICT_ORDINARY" => Some(perry_abi::FN_NON_STRICT_ORDINARY),
        "FN_HAS_NAME" => Some(perry_abi::FN_HAS_NAME),
        "FN_NAME_IMAGE" => Some(perry_abi::FN_NAME_IMAGE),
        "FN_HAS_CONSTRUCTOR_LAYOUT" => Some(perry_abi::FN_HAS_CONSTRUCTOR_LAYOUT),
        "FN_SOURCE_BLOCKS" => Some(perry_abi::FN_SOURCE_BLOCKS),
        "FN_CALL_ENTRY_SHIFT" => Some(perry_abi::FN_CALL_ENTRY_SHIFT),
        _ => None,
    };
    if let Some(value) = constant {
        return Ok(u64::from(value));
    }
    let number = word.replace('_', "");
    let number = number
        .trim_end_matches("u32")
        .trim_end_matches("u16")
        .trim_end_matches("u64");
    if let Some(hex) = number.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        number.parse()
    }
    .map_err(|_| format!("exported info needs an ABI constant or scalar literal, got {word}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declaration(body: &str, info: JsFunctionInfo) -> Value {
        let facts = [
            ("params", u64::from(info.params)),
            ("rest_fixed", u64::from(info.rest_fixed)),
            ("flags", u64::from(info.flags)),
            ("length", u64::from(info.length)),
            ("declared", u64::from(info.declared)),
            (
                "plain_params",
                u64::from(info.plain_params().wrapping_add(1)),
            ),
        ]
        .into_iter()
        .map(|(key, n)| (key.into(), Value::Number(n.to_string())))
        .collect();
        Value::Object(BTreeMap::from([
            ("name".into(), Value::String("Test.member".into())),
            ("body".into(), Value::String(body.into())),
            (
                "source".into(),
                Value::String("perry-runtime-manifest/src/bodies.rs".into()),
            ),
            ("function_info".into(), Value::Object(facts)),
        ]))
    }

    fn base(params: u16) -> JsFunctionInfo {
        unsafe { JsFunctionInfo::from_code(std::ptr::null(), params) }
    }

    #[test]
    fn named_record_matches_real_abi_and_macro_link_name() {
        let mut declarations = vec![declaration(
            "crate::bodies::member",
            base(2)
                .with_declared(1)
                .with_flags(perry_abi::FN_BUILTIN | perry_abi::FN_NON_CONSTRUCTOR),
        )];
        let source = "fn_info!(export static pub MEMBER_INFO = crate::bodies::member, 2; with_declared(1), with_flags(perry_abi::FN_BUILTIN | perry_abi::FN_NON_CONSTRUCTOR));";
        let mut exports = InfoExports::default();
        exports
            .read(
                source,
                "owner.rs",
                env!("CARGO_PKG_NAME"),
                &InfoExports::needed(&declarations),
            )
            .unwrap();
        exports.link(&mut declarations).unwrap();
        let expected = perry_abi::fn_info!(@image_name crate::bodies::member, 2, [with_declared(1), with_flags(perry_abi::FN_BUILTIN | perry_abi::FN_NON_CONSTRUCTOR)]);
        assert_eq!(
            declarations[0].get("info_symbol").and_then(Value::string),
            Some(expected)
        );
    }

    #[test]
    fn imports_do_not_define_owners_and_aliases_share_the_record() {
        let mut declarations = vec![
            declaration("member", base(0)),
            declaration("member", base(0)),
        ];
        let source = "fn_info!(reference export crate::bodies::member, 0); fn_info!(reference import crate::bodies::member, 0);";
        let mut exports = InfoExports::default();
        exports
            .read(
                source,
                "bodies.rs",
                env!("CARGO_PKG_NAME"),
                &InfoExports::needed(&declarations),
            )
            .unwrap();
        assert_eq!(exports.0.len(), 1);
        exports.link(&mut declarations).unwrap();
        assert_eq!(
            declarations[0].get("info_symbol"),
            declarations[1].get("info_symbol")
        );
        assert!(declarations[0].get("info_symbol").is_some());
        assert!(exports
            .read(
                source,
                "second-owner.rs",
                env!("CARGO_PKG_NAME"),
                &InfoExports::needed(&declarations)
            )
            .unwrap_err()
            .contains("duplicate owning"));
    }

    #[test]
    fn native_arguments_and_sentinel_records_keep_their_physical_abi() {
        for (source, info) in [
            ("fn_info!(export native_args crate::bodies::member, 3; with_flags(perry_abi::FN_BUILTIN));", base(0).with_rest_kind(3, perry_abi::FN_REST_NATIVE_ARGS).with_flags(perry_abi::FN_BUILTIN)),
            ("fn_info!(export static pub INFO = sentinel crate::bodies::member, 3);", base(3)),
        ] {
            let mut declarations = vec![declaration("crate::bodies::member", info)];
            let mut exports = InfoExports::default();
            exports.read(source, "owner.rs", env!("CARGO_PKG_NAME"), &InfoExports::needed(&declarations)).unwrap();
            exports.link(&mut declarations).unwrap();
            assert!(declarations[0].get("info_symbol").is_some());
        }
    }

    #[test]
    fn absent_or_different_abi_owner_is_an_error() {
        let mut declarations = vec![declaration("crate::bodies::member", base(2))];
        let mut exports = InfoExports::default();
        exports
            .read(
                "fn_info!(export crate::bodies::member, 1);",
                "owner.rs",
                env!("CARGO_PKG_NAME"),
                &InfoExports::needed(&declarations),
            )
            .unwrap();
        let error = exports.link(&mut declarations).unwrap_err();
        assert!(error.contains("without owning fn_info exports (1)"));
        assert!(error.contains("perry_runtime_manifest::bodies::member"));
    }

    #[test]
    fn package_names_disambiguate_identical_crate_paths() {
        let needed = BTreeSet::from([
            "provider_a::bodies::member".into(),
            "provider_b::bodies::member".into(),
        ]);
        let mut exports = InfoExports::default();
        for package in ["provider-a", "provider-b"] {
            exports
                .read(
                    "fn_info!(export crate::bodies::member, 0);",
                    "bodies.rs",
                    package,
                    &needed,
                )
                .unwrap();
        }
        assert_eq!(exports.0.len(), 2);
        let symbols = exports.0.values().map(|(s, _)| s).collect::<BTreeSet<_>>();
        assert_eq!(symbols.len(), 2);
    }
}
