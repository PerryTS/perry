//! What the realm declaration says about %Global%'s lazily born bindings and the builtin
//! families' links, for the two build scripts that turn it into tables: perry-runtime's rows
//! (`GLOBAL_BINDING_ROWS`, `declared_family_links!`) and the compiler's name list
//! (perry-realm-table `LAZY_GLOBAL_BINDINGS`). One source: nothing here is typed by hand.
use crate::realm_json::Value;
use std::fmt::Write;
use std::path::Path;

/// One %Global% member declared `observed(X)`.
pub struct GlobalBinding {
    pub name: String,
    pub enumerable: bool,
    /// `X` is a namespace (else a function).
    pub namespace: bool,
}

/// The body of the `get [Symbol.species]` accessor that a family's birth defines (returns
/// `this`; perry-runtime install_static.rs `install_builtin_species_accessor`).
const BIRTH_SPECIES_GETTER: &str =
    "crate::object::global_this::install_static::builtin_species_getter_thunk";

fn declarations(manifest: &Value) -> Result<&[Value], String> {
    manifest
        .get("declarations")
        .and_then(Value::array)
        .ok_or_else(|| "realm manifest without declarations".into())
}

fn find<'a>(ds: &'a [Value], name: &str) -> Option<&'a Value> {
    ds.iter()
        .find(|d| d.get("name").and_then(Value::string) == Some(name))
}

fn properties(d: &Value) -> &[Value] {
    d.get("properties").and_then(Value::array).unwrap_or(&[])
}

/// %Global%'s `observed(X)` members, in its own key order. An `observed` member whose target
/// is undeclared or neither a function nor a namespace fails the build.
pub fn observed_global_bindings(manifest: &Value) -> Result<Vec<GlobalBinding>, String> {
    let ds = declarations(manifest)?;
    let global = find(ds, "%Global%").ok_or("no %Global% declaration")?;
    let order = global
        .get("own_key_order")
        .and_then(Value::array)
        .ok_or("%Global% without own_key_order")?;
    let mut out = Vec::new();
    for key in order.iter().filter_map(Value::string) {
        let Some(p) = properties(global).iter().find(|p| {
            p.get("key").and_then(Value::string) == Some(key)
                && p.get("symbol") != Some(&Value::Bool(true))
        }) else {
            return Err(format!("%Global% own key {key} has no member"));
        };
        if p.get("observed") != Some(&Value::Bool(true)) {
            continue;
        }
        let target = p
            .get("value")
            .and_then(|v| v.get("value"))
            .and_then(Value::string)
            .ok_or_else(|| format!("observed binding {key} without a target"))?;
        let decl = find(ds, target)
            .ok_or_else(|| format!("observed binding {key} names undeclared {target}"))?;
        let namespace = match decl.get("kind").and_then(Value::string) {
            Some("namespace") => true,
            Some("function") => false,
            k => {
                return Err(format!(
                    "observed binding {key}: {target} is {k:?}, not a function or namespace"
                ))
            }
        };
        let attrs: u8 = match p.get("attributes") {
            Some(Value::Number(n)) => n
                .parse()
                .map_err(|_| format!("observed binding {key}: attributes {n}"))?,
            _ => return Err(format!("observed binding {key} without attributes")),
        };
        out.push(GlobalBinding {
            name: key.into(),
            enumerable: attrs & 2 != 0,
            namespace,
        });
    }
    if out.is_empty() {
        return Err("%Global% declares no observed(X) member".into());
    }
    Ok(out)
}

/// `isNaN` -> `is_na_n`, `WebAssembly` -> `web_assembly`: the birth fn's name.
fn snake(name: &str) -> String {
    let mut s = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                s.push('_');
            }
            s.push(c.to_ascii_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

/// The family identity a realm name `name` declares: a constructor family row (its realm
/// name; a row %Global% does not bind is `%Name%`), not one of the two root families.
fn family<'a>(rows: &std::collections::BTreeMap<String, Value>, name: &'a str) -> Option<&'a str> {
    (rows.contains_key(name) && name != "Object" && name != "Function")
        .then(|| name.trim_matches('%'))
}

/// perry-runtime's generated sources: `global_bindings.rs` (the `global_bindings!` rows; a row
/// without its `birth_global_<name>` fails to compile) and `family_links.rs`
/// (`declared_family_links!(Name)` = (extends, constructor_extends, species) of each declared
/// constructor: the prototype's declared parent, the constructor's declared [[Prototype]], and
/// whether its declared `@@species` getter is the one its birth defines).
pub fn write_runtime(manifest: &Value, out: &Path) -> Result<(), String> {
    let mut rows = String::from("global_bindings! {\n");
    for b in observed_global_bindings(manifest)? {
        writeln!(
            rows,
            "    {:?}, js_install_global_{}, namespace = {}, enumerable = {}, birth_global_{};",
            b.name,
            b.name,
            b.namespace,
            b.enumerable,
            snake(&b.name)
        )
        .unwrap();
    }
    rows.push_str("}\n");
    std::fs::write(out.join("global_bindings.rs"), rows).map_err(|e| e.to_string())?;

    let ds = declarations(manifest)?;
    let rows = crate::realm_source::families::read(include_str!(
        "../../perry-runtime/src/object/global_this_tables.rs"
    ))?;
    let mut links = String::from("macro_rules! declared_family_links {\n");
    for d in ds {
        let Some(name) = d
            .get("name")
            .and_then(Value::string)
            .and_then(|n| family(&rows, n))
        else {
            continue;
        };
        if d.get("kind").and_then(Value::string) != Some("constructor") {
            continue;
        }
        let ctor = d
            .get("prototype")
            .and_then(Value::string)
            .and_then(|n| family(&rows, n));
        let proto = find(ds, &format!("{name}.prototype"))
            .and_then(|p| p.get("prototype"))
            .and_then(Value::string)
            .and_then(|p| p.strip_suffix(".prototype"))
            .and_then(|n| family(&rows, n));
        let species = properties(d).iter().any(|p| {
            p.get("key").and_then(Value::string) == Some("@@species")
                && p.get("get")
                    .and_then(Value::string)
                    .and_then(|g| find(ds, g))
                    .and_then(|g| g.get("body"))
                    .and_then(Value::string)
                    == Some(BIRTH_SPECIES_GETTER)
        });
        if ctor.is_none() && proto.is_none() && !species {
            continue;
        }
        let id = |f: Option<&str>| {
            f.map_or("None::<BuiltinConstructorIdentity>".to_string(), |f| {
                format!("Some(BuiltinConstructorIdentity::{f})")
            })
        };
        writeln!(
            links,
            "    ({name}) => {{ ({}, {}, {species}) }};",
            id(proto.or(ctor)),
            id(ctor)
        )
        .unwrap();
    }
    links.push_str("    ($other:ident) => { (None::<BuiltinConstructorIdentity>, None::<BuiltinConstructorIdentity>, false) };\n}\n");
    std::fs::write(out.join("family_links.rs"), links).map_err(|e| e.to_string())
}
