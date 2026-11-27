//! Host surfaces (HOSTSURF §1): what a native provider (an embedder's DOM,
//! a native library) adds to the realm, declared in one
//! `host-surface-v1.json` per provider and loaded at compile time.
//!
//! A host surface is more realm declarations from another provider: its
//! objects and members become `RealmEntry` rows named `provider:Name`, whose
//! effects the one interpreter of a declaration runs. Anything a manifest
//! does not state does not happen, and every reachable object must be
//! complete (§1.3): a partial manifest is a load error, never a degraded
//! mode. Every violation below is an error the compile reports.
//!
//! Names in a manifest: a bare `Name` is the provider's own node; the
//! static realm's nodes are spelled `perry-realm:Name` (`perry-realm:%Global%`,
//! `perry-realm:Object.prototype`, `perry-realm:TypeError`); another
//! provider's node is `other:Name`. Loaded, a provider node is named
//! `provider:Name` and a realm node keeps its static name.
use super::*;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

pub const HOST_SURFACE_FORMAT: &str = "host-surface-v1";
/// The provider name of the static realm (`perry_realm_table::REALM`).
pub const REALM_PROVIDER: &str = "perry-realm";
/// The file a target runtime ships its host surface in.
pub const HOST_SURFACE_FILE: &str = "host-surface-v1.json";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostSurface {
    pub format: String,
    pub provider: String,
    /// The provider's number in the run-time prototype registry
    /// (`perry_host_register_prototype_v1(provider_id, object_index, ..)`,
    /// `object_index` = the index into `objects`); unique per compile.
    pub provider_id: u32,
    /// The provider's source revision, recorded in the facts dump.
    #[serde(default)]
    pub build: String,
    /// The targets whose link includes this provider (`*`: every target).
    pub targets: Vec<String>,
    #[serde(default)]
    pub extends: Vec<HostExtends>,
    pub binding: HostBinding,
    #[serde(default)]
    pub realm: Option<HostRealm>,
    #[serde(default)]
    pub pools: Vec<HostPool>,
    #[serde(default)]
    pub objects: Vec<HostObject>,
    /// Own keys the provider puts on the realm's global object.
    #[serde(default)]
    pub globals: Vec<HostMember>,
    #[serde(default)]
    pub births: Vec<HostBirth>,
    #[serde(default)]
    pub tags: Vec<HostTags>,
}

/// The only way to add keys to another provider's objects.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostExtends {
    pub provider: String,
    pub objects: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HostBinding {
    pub abi: String,
    pub kind: HostBindingKind,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub shim: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HostBindingKind {
    /// Every member is one operation of one endpoint (`{"operation": "k"}`).
    FunctionEndpoint,
    /// Every member is its own C symbol (`{"symbol": "name"}`).
    Symbol,
}

/// `realm.global`: the interface the global object is an instance of; its
/// prototype continues `%Global%`'s [[Prototype]] chain.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostRealm {
    pub global: String,
    pub complete: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPool {
    pub name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostObjectKind {
    /// An interface prototype object (`Node.prototype`).
    Prototype,
    /// An interface object (`Node`, `HTMLElement`).
    Interface,
    /// A namespace object.
    Namespace,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostObject {
    pub name: String,
    /// Absent on an entry that adds keys to another provider's object.
    #[serde(default)]
    pub kind: Option<HostObjectKind>,
    #[serde(default)]
    pub interface: Option<String>,
    /// Its [[Prototype]]: a node name or `null`; absent on an extension.
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub members: Vec<HostMember>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HostMemberKind {
    Method,
    Accessor,
    Data,
    NamedGetterMethod,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HostEntry {
    Operation(String),
    Symbol(String),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostArg {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub idl: Option<String>,
}

/// One function part of a member: a method, a getter or a setter.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostFunction {
    pub entry: HostEntry,
    #[serde(default)]
    pub args: Vec<HostArg>,
    pub effects: Vec<RealmEffect>,
    #[serde(default)]
    pub throws: Vec<String>,
}

/// What a `data` member holds.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HostValue {
    Prim(RealmPrim),
    /// An object of a provider (an interface object, a prototype).
    Object(String),
    /// The one instance of a birth kind (`document`: `Document`).
    Birth(String),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostMember {
    pub key: String,
    pub kind: HostMemberKind,
    pub attributes: u8,
    #[serde(default)]
    pub length: Option<u32>,
    #[serde(default)]
    pub entry: Option<HostEntry>,
    #[serde(default)]
    pub args: Vec<HostArg>,
    #[serde(default)]
    pub effects: Vec<RealmEffect>,
    #[serde(default)]
    pub throws: Vec<String>,
    #[serde(default)]
    pub get: Option<HostFunction>,
    #[serde(default)]
    pub set: Option<HostFunction>,
    #[serde(default)]
    pub value: Option<HostValue>,
}

/// Instance kinds the provider makes without a program call (§1.4.3).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostBirth {
    pub name: String,
    /// Interface names (`HostObject::interface` of prototype objects).
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub pools: Vec<String>,
}

/// A tag relation (§1.4.4): string value to the prototype a fresh object
/// made for it has; `other` for every string the map does not name.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostTags {
    pub name: String,
    pub map: BTreeMap<String, String>,
    pub other: String,
}

/// What a site needs to call a member directly (§1.5), per function entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostCall {
    pub provider: &'static str,
    pub entry: HostEntry,
    pub throws: Vec<&'static str>,
}

#[derive(Clone, Debug)]
pub struct HostBirthKind {
    pub name: &'static str,
    /// The prototypes of its interfaces: its chain is one of theirs.
    pub protos: Vec<&'static str>,
    pub pools: Vec<PoolName>,
}

#[derive(Clone, Debug)]
pub struct HostTagRelation {
    pub map: BTreeMap<String, &'static str>,
    pub other: &'static str,
}

/// The merged, validated host surfaces of one compile: rows beside the
/// static realm, leaked once per compile.
#[derive(Clone, Debug, Default)]
pub struct HostTable {
    pub providers: Vec<(&'static str, String)>,
    pub entries: Vec<RealmEntry>,
    /// Own keys the providers add to `%Global%`.
    pub global_own: Vec<RealmOwn>,
    /// `%Global%`'s [[Prototype]] when a provider declares `realm.global`.
    pub global_proto: Option<&'static str>,
    pub object_kinds: BTreeMap<&'static str, HostObjectKind>,
    /// Interface name (`provider:Node`) to its prototype object.
    pub interfaces: BTreeMap<&'static str, &'static str>,
    pub births: BTreeMap<&'static str, HostBirthKind>,
    pub pools: BTreeSet<PoolName>,
    pub tags: BTreeMap<&'static str, HostTagRelation>,
    pub calls: BTreeMap<&'static str, HostCall>,
    /// Own-key attributes of host members, by holder and key.
    pub attributes: BTreeMap<(&'static str, &'static str), u8>,
    /// `named-getter-method` members (by member entry name).
    pub named_getters: BTreeSet<&'static str>,
}

impl HostTable {
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
    pub fn entry(&self, name: &str) -> Option<&RealmEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

/// A build input that adds globals the static realm does not declare: a
/// native library whose package declares `perry.hostGlobals`, or a target
/// whose embedder installs a host surface. `by` names it in the error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRequirement {
    pub by: String,
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

pub fn parse(json: &str, origin: &str) -> Result<HostSurface, String> {
    let surface: HostSurface =
        serde_json::from_str(json).map_err(|e| format!("{origin}: host surface: {e}"))?;
    if surface.format != HOST_SURFACE_FORMAT {
        return Err(format!(
            "{origin}: host surface format `{}`; expected `{HOST_SURFACE_FORMAT}`",
            surface.format
        ));
    }
    if surface.provider.is_empty()
        || surface.provider.contains(':')
        || surface.provider == REALM_PROVIDER
    {
        return Err(format!(
            "{origin}: provider name `{}` must be non-empty, without `:`, and not `{REALM_PROVIDER}`",
            surface.provider
        ));
    }
    if surface.targets.is_empty() {
        return Err(format!("{origin}: `targets` names no target"));
    }
    Ok(surface)
}

/// Whether a surface's link includes `target`.
pub fn for_target(surface: &HostSurface, target: &str) -> bool {
    surface.targets.iter().any(|t| t == "*" || t == target)
}

/// GUARD (HOSTSURF §0.2): whole-program facts read `%Global%` as complete.
/// A build whose host adds globals is only sound when a loaded surface
/// declares them (it extends `perry-realm:%Global%`). Otherwise a global
/// the host adds would read `undefined` and its sites would be proven
/// dead or throwing: the compile is refused, never answered with an
/// unknown global.
pub fn check_required(reqs: &[HostRequirement], loaded: &[HostSurface]) -> Result<(), String> {
    if reqs.is_empty() {
        return Ok(());
    }
    let declares_global = loaded.iter().any(|s| {
        s.extends
            .iter()
            .any(|e| e.provider == REALM_PROVIDER && e.objects.iter().any(|o| o == "%Global%"))
    });
    if declares_global {
        return Ok(());
    }
    let by: Vec<&str> = reqs.iter().map(|r| r.by.as_str()).collect();
    Err(format!(
        "whole-program facts: {} adds globals to the realm, but no host surface ({HOST_SURFACE_FORMAT}) \
         that extends `{REALM_PROVIDER}:%Global%` is loaded. The realm's global object would read as \
         complete without them. Load the provider's host surface (package.json `perry.hostSurface`, \
         `--host-surface`, or the target runtime's {HOST_SURFACE_FILE}), or compile with PERRY_PROGRAM_FACTS=0.",
        by.join(", ")
    ))
}

/// What the static realm offers the loader: its node names and the own
/// keys of `%Global%` (a host global may not shadow one).
pub struct StaticRealm<'a> {
    pub has: &'a dyn Fn(&str) -> bool,
    pub global_has: &'a dyn Fn(&str) -> bool,
}

struct Loader<'a> {
    realm: &'a StaticRealm<'a>,
    /// Every provider node: object, member entry, birth (loaded names).
    names: BTreeSet<String>,
    providers: BTreeSet<String>,
}

impl Loader<'_> {
    /// Resolves a manifest name of `provider` to its loaded name.
    fn resolve(&self, provider: &str, name: &str, at: &str) -> Result<String, String> {
        let full = match name.split_once(':') {
            Some((REALM_PROVIDER, rest)) => {
                if (self.realm.has)(rest) {
                    return Ok(rest.to_string());
                }
                return Err(format!("{at}: `{name}` is not a node of the static realm"));
            }
            Some((p, _)) if !self.providers.contains(p) => {
                return Err(format!("{at}: `{name}` names provider `{p}`, which is not loaded"));
            }
            Some(_) => name.to_string(),
            None => format!("{provider}:{name}"),
        };
        if self.names.contains(&full) {
            Ok(full)
        } else {
            Err(format!("{at}: `{name}` names no node"))
        }
    }

    fn pool(&self, provider: &str, name: &str, at: &str, pools: &BTreeSet<PoolName>) -> Result<PoolName, String> {
        let full = if name.contains(':') {
            name.to_string()
        } else {
            format!("{provider}:{name}")
        };
        pools
            .iter()
            .find(|p| p.0 == full)
            .copied()
            .ok_or_else(|| format!("{at}: pool `{name}` is not declared"))
    }
}

/// Rewrites every name an effect list holds to its loaded name, checking
/// that pools, `is` filters, tag relations and argument indexes resolve.
fn load_effects(
    l: &Loader<'_>,
    provider: &str,
    at: &str,
    effects: &[RealmEffect],
    args: usize,
    pools: &BTreeSet<PoolName>,
    tags: &BTreeSet<String>,
) -> Result<Vec<RealmEffect>, String> {
    let slot = |s: &RealmSlot| -> Result<RealmSlot, String> {
        let root = match s.root {
            RealmSlotRoot::Pool(p) => RealmSlotRoot::Pool(l.pool(provider, p.0, at, pools)?),
            r => r,
        };
        let steps = s
            .steps
            .iter()
            .map(|step| match step {
                RealmSlotStep::Is(n) => Ok(RealmSlotStep::Is(Cow::Owned(l.resolve(provider, n, at)?))),
                other => Ok(other.clone()),
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(RealmSlot { root, steps: Cow::Owned(steps) })
    };
    fn operand(
        o: &RealmOperand,
        l: &Loader<'_>,
        provider: &str,
        at: &str,
        args: usize,
        tags: &BTreeSet<String>,
        slot: &dyn Fn(&RealmSlot) -> Result<RealmSlot, String>,
    ) -> Result<RealmOperand, String> {
        let op = |o: &RealmOperand| operand(o, l, provider, at, args, tags, slot);
        let ops = |v: &[RealmOperand]| v.iter().map(op).collect::<Result<Vec<_>, String>>();
        let name = |n: &str| -> Result<Cow<'static, str>, String> {
            if n == "null" {
                Ok(Cow::Borrowed("null"))
            } else {
                Ok(Cow::Owned(l.resolve(provider, n, at)?))
            }
        };
        Ok(match o {
            RealmOperand::Slot(s) => RealmOperand::Slot(slot(s)?),
            RealmOperand::Fresh { proto, elems, fields, slots } => RealmOperand::Fresh {
                proto: name(proto)?,
                elems: Cow::Owned(ops(elems)?),
                fields: Cow::Owned(
                    fields
                        .iter()
                        .map(|f| Ok(RealmField { key: f.key.clone(), value: op(&f.value)? }))
                        .collect::<Result<Vec<_>, String>>()?,
                ),
                slots: Cow::Owned(
                    slots
                        .iter()
                        .map(|f| Ok(RealmField { key: f.key.clone(), value: op(&f.value)? }))
                        .collect::<Result<Vec<_>, String>>()?,
                ),
            },
            RealmOperand::Promise { value, reject } => RealmOperand::Promise {
                value: Cow::Owned(ops(value)?),
                reject: Cow::Owned(ops(reject)?),
            },
            RealmOperand::Native(n) => RealmOperand::Native(name(n)?),
            RealmOperand::Namespace(n) => RealmOperand::Namespace(name(n)?),
            RealmOperand::Species { of, args: a, elems } => RealmOperand::Species {
                of: slot(of)?,
                args: Cow::Owned(ops(a)?),
                elems: Cow::Owned(elems.iter().map(slot).collect::<Result<Vec<_>, String>>()?),
            },
            RealmOperand::Union(m) => RealmOperand::Union(Cow::Owned(ops(m)?)),
            RealmOperand::FreshByTag { arg, tags: t } => {
                if usize::from(*arg) >= args {
                    return Err(format!("{at}: fresh_by_tag reads argument {arg} of {args}"));
                }
                let full = if t.contains(':') { t.to_string() } else { format!("{provider}:{t}") };
                if !tags.contains(&full) {
                    return Err(format!("{at}: tag relation `{t}` is not declared"));
                }
                RealmOperand::FreshByTag { arg: *arg, tags: Cow::Owned(full) }
            }
            // An unknown value is unknown code: a host member says `open`.
            RealmOperand::Unknown => {
                return Err(format!("{at}: `unknown` value; a host member that runs unknown code declares `open`"))
            }
            other => other.clone(),
        })
    }
    let mut out = Vec::new();
    for e in effects {
        let op = |o: &RealmOperand| operand(o, l, provider, at, args, tags, &slot);
        out.push(match e {
            RealmEffect::Reads(s) => {
                RealmEffect::Reads(Cow::Owned(s.iter().map(&slot).collect::<Result<Vec<_>, String>>()?))
            }
            RealmEffect::Coerce { slot: s, protocol } => {
                RealmEffect::Coerce { slot: slot(s)?, protocol: *protocol }
            }
            RealmEffect::Calls { callee, this, args: a } => RealmEffect::Calls {
                callee: slot(callee)?,
                this: op(this)?,
                args: Cow::Owned(a.iter().map(op).collect::<Result<Vec<_>, String>>()?),
            },
            RealmEffect::Stores { src, dst } => RealmEffect::Stores { src: slot(src)?, dst: slot(dst)? },
            RealmEffect::Escapes(s) => RealmEffect::Escapes(slot(s)?),
            RealmEffect::AsyncResource(s) => RealmEffect::AsyncResource(slot(s)?),
            RealmEffect::StoresBytes(s) => RealmEffect::StoresBytes(slot(s)?),
            RealmEffect::Identity(s) => RealmEffect::Identity(slot(s)?),
            RealmEffect::Returns(o) => RealmEffect::Returns(op(o)?),
            RealmEffect::StoresFresh | RealmEffect::Open => e.clone(),
        });
    }
    // Pools hold values: never a coercion, call or escape of a pool.
    for e in &out {
        let bad = match e {
            RealmEffect::Calls { callee, .. } => matches!(callee.root, RealmSlotRoot::Pool(_)),
            RealmEffect::Escapes(s) => matches!(s.root, RealmSlotRoot::Pool(_)),
            _ => false,
        };
        if bad {
            return Err(format!("{at}: a pool is a store of values; it is never called or escaped"));
        }
    }
    let names = |n: &str| n == "null" || (l.realm.has)(n) || l.names.contains(n);
    validate_effects(at, &out, &names)?;
    Ok(out)
}

fn own_value(kind: RealmPrim) -> RealmOwnValue {
    match kind {
        RealmPrim::Undefined => RealmOwnValue::Undefined,
        RealmPrim::Null => RealmOwnValue::Null,
        RealmPrim::Bool => RealmOwnValue::Boolean,
        RealmPrim::Num => RealmOwnValue::Number,
        RealmPrim::Str => RealmOwnValue::String,
        RealmPrim::Symbol => RealmOwnValue::Symbol,
        _ => RealmOwnValue::Other,
    }
}

fn holder_name(l: &Loader<'_>, provider: &str, holder: &str) -> Result<String, String> {
    if holder == "%Global%" {
        return Ok(holder.to_string());
    }
    l.resolve(provider, holder, &format!("{provider}: object `{holder}`"))
}

struct Ctx<'a, 'b> {
    l: &'a Loader<'b>,
    s: &'a HostSurface,
    pl: &'static str,
    tag_names: &'a BTreeSet<String>,
}

/// One function part (method, getter, setter): its table row and its call.
fn function(
    c: &Ctx<'_, '_>,
    t: &mut HostTable,
    name: String,
    kind: RealmKind,
    f: &HostFunction,
    length: u32,
) -> Result<&'static str, String> {
    let p = c.s.provider.as_str();
    let fat = format!("{p}: {name}");
    let ok = matches!(
        (&f.entry, c.s.binding.kind),
        (HostEntry::Operation(_), HostBindingKind::FunctionEndpoint)
            | (HostEntry::Symbol(_), HostBindingKind::Symbol)
    );
    if !ok {
        return Err(format!("{fat}: its entry does not match the provider's binding kind"));
    }
    if f.effects.is_empty() {
        return Err(format!(
            "{fat}: declares no effects (a member that runs unknown code declares `open`)"
        ));
    }
    let args = f.args.len().max(length as usize);
    let effects = load_effects(c.l, p, &fat, &f.effects, args, &t.pools, c.tag_names)?;
    let throws = f
        .throws
        .iter()
        .map(|e| c.l.resolve(p, e, &fat).map(leak))
        .collect::<Result<Vec<_>, String>>()?;
    let n: &'static str = leak(name);
    t.entries.push(RealmEntry {
        name: n,
        kind,
        births: &[],
        module: None,
        facts: "{}",
        effects: Box::leak(effects.into_boxed_slice()),
        own: &[],
        proto: Some("Function.prototype"),
        value: None,
    });
    t.calls.insert(n, HostCall { provider: c.pl, entry: f.entry.clone(), throws });
    Ok(n)
}

/// The own-key rows of one object's (or the global's) members.
fn member_rows(
    c: &Ctx<'_, '_>,
    t: &mut HostTable,
    holder_full: &str,
    base: &str,
    members: &[HostMember],
) -> Result<Vec<RealmOwn>, String> {
    let p = c.s.provider.as_str();
    let mut keys = BTreeSet::new();
    let mut row = Vec::new();
    for m in members {
        let at = format!("{p}: {base}.{}", m.key);
        if m.key.is_empty() || !keys.insert(m.key.clone()) {
            return Err(format!("{at}: key is empty or declared twice on the object"));
        }
        if m.attributes > 7 {
            return Err(format!("{at}: attributes {} is not a [[W]][[E]][[C]] bit set", m.attributes));
        }
        let key: &'static str = leak(m.key.clone());
        let enumerable = m.attributes & 2 != 0;
        let value = match m.kind {
            HostMemberKind::Method | HostMemberKind::NamedGetterMethod => {
                if m.get.is_some() || m.set.is_some() || m.value.is_some() {
                    return Err(format!("{at}: a method has no get/set/value"));
                }
                let entry = m.entry.clone().ok_or_else(|| format!("{at}: a method names its entry"))?;
                let f = HostFunction {
                    entry,
                    args: m.args.clone(),
                    effects: m.effects.clone(),
                    throws: m.throws.clone(),
                };
                let name = format!("{base}.{}", m.key);
                let n = function(c, t, name, RealmKind::Method, &f, m.length.unwrap_or(0))?;
                if m.kind == HostMemberKind::NamedGetterMethod {
                    t.named_getters.insert(n);
                }
                RealmOwnValue::Reference(n)
            }
            HostMemberKind::Accessor => {
                if m.entry.is_some() || m.value.is_some() || !m.effects.is_empty() {
                    return Err(format!("{at}: an accessor declares its parts under get/set"));
                }
                if m.get.is_none() && m.set.is_none() {
                    return Err(format!("{at}: an accessor has a get or a set"));
                }
                let get = match &m.get {
                    Some(g) => Some(function(c, t, format!("{base}.{}.<get>", m.key), RealmKind::Getter, g, 0)?),
                    None => None,
                };
                let set = match &m.set {
                    Some(g) => Some(function(c, t, format!("{base}.{}.<set>", m.key), RealmKind::Setter, g, 1)?),
                    None => None,
                };
                RealmOwnValue::Accessor { get, set }
            }
            HostMemberKind::Data => {
                if m.entry.is_some() || m.get.is_some() || m.set.is_some() || !m.effects.is_empty() {
                    return Err(format!("{at}: a data member holds a value only"));
                }
                match m.value.as_ref().ok_or_else(|| format!("{at}: a data member names its value"))? {
                    HostValue::Prim(k) => own_value(*k),
                    HostValue::Object(n) | HostValue::Birth(n) => {
                        RealmOwnValue::Reference(leak(c.l.resolve(p, n, &at)?))
                    }
                }
            }
        };
        t.attributes.insert((leak(holder_full.to_string()), key), m.attributes);
        row.push(RealmOwn { key, symbol: false, enumerable, value });
    }
    Ok(row)
}

/// Loads every surface of one compile into a `HostTable`, or the first error.
pub fn load(surfaces: &[HostSurface], realm: &StaticRealm<'_>) -> Result<HostTable, String> {
    let mut l = Loader { realm, names: BTreeSet::new(), providers: BTreeSet::new() };
    let mut ids = BTreeSet::new();
    for s in surfaces {
        if !l.providers.insert(s.provider.clone()) {
            return Err(format!("host surface: provider `{}` is loaded twice", s.provider));
        }
        if !ids.insert(s.provider_id) {
            return Err(format!(
                "host surface: provider `{}` reuses provider_id {}",
                s.provider, s.provider_id
            ));
        }
    }
    // Pass 1: every node name (objects, member entries, births), so that
    // references between providers and forward references resolve.
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for s in surfaces {
        let p = &s.provider;
        for o in &s.objects {
            if o.name.contains(':') {
                continue;
            }
            let full = format!("{p}:{}", o.name);
            if owner.insert(full.clone(), p.clone()).is_some() {
                return Err(format!("{p}: object `{}` is declared twice", o.name));
            }
            l.names.insert(full);
        }
        for b in &s.births {
            let full = format!("{p}:{}", b.name);
            if !l.names.insert(full) {
                return Err(format!("{p}: birth `{}` clashes with another node", b.name));
            }
        }
    }
    let mut t = HostTable::default();
    let mut tag_names = BTreeSet::new();
    for s in surfaces {
        let p = &s.provider;
        for pool in &s.pools {
            if !t.pools.insert(PoolName(leak(format!("{p}:{}", pool.name)))) {
                return Err(format!("{p}: pool `{}` is declared twice", pool.name));
            }
        }
        for tg in &s.tags {
            if !tag_names.insert(format!("{p}:{}", tg.name)) {
                return Err(format!("{p}: tag relation `{}` is declared twice", tg.name));
            }
        }
    }
    // Member entry names (`provider:Obj.key`, `.key.<get>`), declared before
    // effects are loaded: a fresh(..)/native(..) may name a member function.
    let mut member_names = Vec::new();
    for s in surfaces {
        let holders = s
            .objects
            .iter()
            .map(|o| (o.name.as_str(), &o.members))
            .chain(std::iter::once(("%Global%", &s.globals)));
        for (holder, members) in holders {
            let h = holder_name(&l, &s.provider, holder)?;
            let base = if h == "%Global%" { format!("{}:%Global%", s.provider) } else { h };
            for m in members.iter() {
                match m.kind {
                    HostMemberKind::Method | HostMemberKind::NamedGetterMethod => {
                        member_names.push(format!("{base}.{}", m.key))
                    }
                    HostMemberKind::Accessor => {
                        if m.get.is_some() {
                            member_names.push(format!("{base}.{}.<get>", m.key));
                        }
                        if m.set.is_some() {
                            member_names.push(format!("{base}.{}.<set>", m.key));
                        }
                    }
                    HostMemberKind::Data => {}
                }
            }
        }
    }
    for n in member_names {
        if !l.names.insert(n.clone()) {
            return Err(format!("host surface: member `{n}` is declared twice"));
        }
    }

    // Pass 2: objects, members, globals, births, tags.
    let mut own: BTreeMap<String, Vec<RealmOwn>> = BTreeMap::new();
    let mut protos: BTreeMap<String, Option<&'static str>> = BTreeMap::new();
    let mut global_set_by: Option<String> = None;
    for s in surfaces {
        let p = s.provider.as_str();
        let pl: &'static str = leak(p.to_string());
        t.providers.push((pl, s.build.clone()));
        let extends = |prov: &str, obj: &str| {
            s.extends
                .iter()
                .any(|e| e.provider == prov && e.objects.iter().any(|o| o == obj))
        };
        for e in &s.extends {
            if e.provider == p {
                return Err(format!("{p}: `extends` names the provider itself"));
            }
            if e.provider != REALM_PROVIDER && !l.providers.contains(&e.provider) {
                return Err(format!("{p}: `extends` names provider `{}`, which is not loaded", e.provider));
            }
        }
        if s.binding.kind == HostBindingKind::FunctionEndpoint
            && (s.binding.endpoint.is_none() || s.binding.shim.is_none())
        {
            return Err(format!("{p}: a function-endpoint binding names its endpoint and shim"));
        }
        let c = Ctx { l: &l, s, pl, tag_names: &tag_names };
        for o in &s.objects {
            let at = format!("{p}: object `{}`", o.name);
            let full = holder_name(&l, p, &o.name)?;
            if let Some((other, local)) = o.name.split_once(':') {
                // Keys onto another provider's object: an extension only.
                if !extends(other, local) {
                    return Err(format!("{at}: adds keys to `{}` without an `extends` entry for it", o.name));
                }
                if o.kind.is_some() || o.parent.is_some() || o.interface.is_some() {
                    return Err(format!("{at}: an extension adds members only (no kind, parent or interface)"));
                }
                let rows = member_rows(&c, &mut t, &full, &full, &o.members)?;
                own.entry(full).or_default().extend(rows);
                continue;
            }
            let kind = o.kind.ok_or_else(|| format!("{at}: names its kind"))?;
            if !o.complete {
                return Err(format!(
                    "{at}: is not complete; a host surface declares every own key of every object (§1.3)"
                ));
            }
            let parent = o.parent.as_deref().ok_or_else(|| format!("{at}: names its parent (or null)"))?;
            let proto = if parent == "null" {
                None
            } else {
                let r = l.resolve(p, parent, &at)?;
                let realm_ok = r == "Object.prototype" || r == "Function.prototype";
                if !(realm_ok || owner.contains_key(&r)) {
                    return Err(format!(
                        "{at}: parent `{parent}` is neither a provider object nor Object.prototype/Function.prototype"
                    ));
                }
                Some(leak(r))
            };
            let n = leak(full.clone());
            t.object_kinds.insert(n, kind);
            if let Some(iface) = &o.interface {
                if kind == HostObjectKind::Prototype
                    && t.interfaces.insert(leak(format!("{p}:{iface}")), n).is_some()
                {
                    return Err(format!("{at}: interface `{iface}` has two prototype objects"));
                }
            }
            protos.insert(full.clone(), proto);
            let rows = member_rows(&c, &mut t, &full, &full, &o.members)?;
            own.entry(full).or_default().extend(rows);
        }
        // Globals.
        if !s.globals.is_empty() || s.realm.is_some() {
            if !extends(REALM_PROVIDER, "%Global%") {
                return Err(format!(
                    "{p}: declares globals without `extends` {{provider: {REALM_PROVIDER}, objects: [%Global%]}}"
                ));
            }
        }
        let base = format!("{p}:%Global%");
        let rows = member_rows(&c, &mut t, "%Global%", &base, &s.globals)?;
        for r in rows {
            if (realm.global_has)(r.key) || t.global_own.iter().any(|g| g.key == r.key) {
                return Err(format!("{p}: global `{}` is already declared by another provider", r.key));
            }
            t.global_own.push(r);
        }
        if let Some(rg) = &s.realm {
            if !rg.complete {
                return Err(format!("{p}: realm.global is not complete (§1.3)"));
            }
            if let Some(prev) = &global_set_by {
                return Err(format!("{p}: realm.global is already set by `{prev}`"));
            }
            global_set_by = Some(p.to_string());
            let iface = format!("{p}:{}", rg.global);
            let proto = *t
                .interfaces
                .get(iface.as_str())
                .ok_or_else(|| format!("{p}: realm.global `{}` names no interface prototype", rg.global))?;
            t.global_proto = Some(proto);
        }
        for b in &s.births {
            let at = format!("{p}: birth `{}`", b.name);
            if b.interfaces.is_empty() {
                return Err(format!("{at}: names no interface"));
            }
            let protos = b
                .interfaces
                .iter()
                .map(|i| {
                    let full = if i.contains(':') { i.clone() } else { format!("{p}:{i}") };
                    t.interfaces
                        .get(full.as_str())
                        .copied()
                        .ok_or_else(|| format!("{at}: interface `{i}` has no prototype object"))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let pools = b
                .pools
                .iter()
                .map(|n| l.pool(p, n, &at, &t.pools))
                .collect::<Result<Vec<_>, String>>()?;
            let n = leak(format!("{p}:{}", b.name));
            t.entries.push(RealmEntry {
                name: n,
                kind: RealmKind::BirthSite,
                births: Box::leak(protos.clone().into_boxed_slice()),
                module: None,
                facts: "{}",
                effects: &[],
                own: &[],
                proto: if protos.len() == 1 { Some(protos[0]) } else { None },
                value: None,
            });
            t.births.insert(n, HostBirthKind { name: n, protos, pools });
        }
        for tg in &s.tags {
            let at = format!("{p}: tags `{}`", tg.name);
            let proto = |n: &str| -> Result<&'static str, String> {
                let r = l.resolve(p, n, &at)?;
                if !t.object_kinds.get(r.as_str()).is_some_and(|k| *k == HostObjectKind::Prototype) {
                    return Err(format!("{at}: `{n}` is not a prototype object"));
                }
                Ok(leak(r))
            };
            let map = tg
                .map
                .iter()
                .map(|(k, v)| Ok((k.clone(), proto(v)?)))
                .collect::<Result<BTreeMap<_, _>, String>>()?;
            let other = proto(&tg.other)?;
            t.tags.insert(leak(format!("{p}:{}", tg.name)), HostTagRelation { map, other });
        }
    }
    // Object rows, with every provider's members merged in.
    for (name, proto) in protos {
        let rows = own.remove(&name).unwrap_or_default();
        let n: &'static str = t.object_kinds.keys().find(|k| **k == name).copied().unwrap_or_else(|| leak(name));
        let kind = match t.object_kinds[n] {
            HostObjectKind::Prototype => RealmKind::Prototype,
            HostObjectKind::Interface | HostObjectKind::Namespace => RealmKind::Namespace,
        };
        t.entries.push(RealmEntry {
            name: n,
            kind,
            births: &[],
            module: None,
            facts: "{}",
            effects: &[],
            own: Box::leak(rows.into_boxed_slice()),
            proto,
            value: None,
        });
    }
    if let Some(name) = own.keys().next() {
        return Err(format!("host surface: keys added to `{name}`, which no loaded provider declares"));
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOY: &str = r#"{
      "format": "host-surface-v1", "provider": "toy", "provider_id": 1, "build": "t1", "targets": ["*"],
      "extends": [{"provider": "perry-realm", "objects": ["%Global%"]}],
      "binding": {"abi": "perry-binding-call-v1", "kind": "function-endpoint",
                  "endpoint": "toy::Dispatch", "shim": "toy_call_v1"},
      "realm": {"global": "Window", "complete": true},
      "pools": [{"name": "nodes"}],
      "tags": [{"name": "html", "map": {"p": "P.prototype"}, "other": "Element.prototype"}],
      "objects": [
        {"name": "Node.prototype", "kind": "prototype", "interface": "Node", "parent": "perry-realm:Object.prototype",
         "complete": true, "members": [
          {"key": "appendChild", "kind": "method", "attributes": 7, "length": 1, "entry": {"operation": "kAppend"},
           "args": [{"idl": "Node"}],
           "effects": [{"stores": {"src": {"root": {"arg": 0}}, "dst": {"root": {"pool": "nodes"}}}},
                       {"returns": {"slot": {"root": {"arg": 0}}}}],
           "throws": ["perry-realm:TypeError"]},
          {"key": "textContent", "kind": "accessor", "attributes": 6,
           "get": {"entry": {"operation": "kGetText"}, "effects": [{"returns": {"union": [{"prim": ["null"]}, {"prim": ["str"]}]}}]},
           "set": {"entry": {"operation": "kSetText"}, "args": [{"idl": "DOMString?"}],
                   "effects": [{"coerce": {"slot": {"root": {"arg": 0}}, "protocol": "to_primitive"}}, "stores_fresh"]}}]},
        {"name": "Element.prototype", "kind": "prototype", "interface": "Element", "parent": "Node.prototype",
         "complete": true, "members": []},
        {"name": "P.prototype", "kind": "prototype", "interface": "P", "parent": "Element.prototype",
         "complete": true, "members": []},
        {"name": "Document.prototype", "kind": "prototype", "interface": "Document", "parent": "Node.prototype",
         "complete": true, "members": [
          {"key": "getElementById", "kind": "method", "attributes": 7, "length": 1, "entry": {"operation": "kById"},
           "effects": [{"coerce": {"slot": {"root": {"arg": 0}}, "protocol": "to_primitive"}},
                       {"returns": {"union": [{"prim": ["null"]},
                         {"slot": {"root": {"pool": "nodes"}, "steps": [{"is": "Element.prototype"}]}}]}}]},
          {"key": "createElement", "kind": "method", "attributes": 7, "length": 1, "entry": {"operation": "kCreate"},
           "effects": [{"coerce": {"slot": {"root": {"arg": 0}}, "protocol": "to_primitive"}},
                       {"returns": {"fresh_by_tag": {"arg": 0, "tags": "html"}}}]}]},
        {"name": "Window.prototype", "kind": "prototype", "interface": "Window", "parent": "perry-realm:Object.prototype",
         "complete": true, "members": []}
      ],
      "globals": [{"key": "document", "kind": "data", "attributes": 4, "value": {"birth": "Document"}}],
      "births": [{"name": "Document", "interfaces": ["Document"], "pools": ["nodes"]},
                 {"name": "Element+", "interfaces": ["Element", "P"], "pools": ["nodes"]}]
    }"#;

    fn realm() -> StaticRealm<'static> {
        StaticRealm {
            has: &|n| matches!(n, "Object.prototype" | "Function.prototype" | "TypeError" | "%Global%"),
            global_has: &|k| matches!(k, "Object" | "globalThis"),
        }
    }

    fn load_json(jsons: &[&str]) -> Result<HostTable, String> {
        let s = jsons
            .iter()
            .map(|j| parse(j, "test"))
            .collect::<Result<Vec<_>, String>>()?;
        load(&s, &realm())
    }

    fn with(edit: impl Fn(&mut serde_json::Value)) -> String {
        let mut v: serde_json::Value = serde_json::from_str(TOY).unwrap();
        edit(&mut v);
        v.to_string()
    }

    fn err(edit: impl Fn(&mut serde_json::Value), needle: &str) {
        let j = with(edit);
        let e = load_json(&[&j]).expect_err("must be refused");
        assert!(e.contains(needle), "{e}");
    }

    #[test]
    fn toy_manifest_loads() {
        let t = load_json(&[TOY]).unwrap();
        let node = t.entry("toy:Node.prototype").unwrap();
        assert_eq!(node.proto, Some("Object.prototype"));
        assert!(node.own.iter().any(|o| o.key == "appendChild"
            && o.value == RealmOwnValue::Reference("toy:Node.prototype.appendChild")));
        let append = t.entry("toy:Node.prototype.appendChild").unwrap();
        assert!(matches!(append.effects[0], RealmEffect::Stores { ref dst, .. }
            if dst.root == RealmSlotRoot::Pool(PoolName("toy:nodes"))));
        assert_eq!(t.calls["toy:Node.prototype.appendChild"].entry, HostEntry::Operation("kAppend".into()));
        assert_eq!(t.calls["toy:Node.prototype.appendChild"].throws, ["TypeError"]);
        assert!(t.entry("toy:Node.prototype.textContent.<get>").is_some());
        assert!(t.entry("toy:Node.prototype.textContent.<set>").is_some());
        assert_eq!(t.global_proto, Some("toy:Window.prototype"));
        assert_eq!(t.global_own[0].value, RealmOwnValue::Reference("toy:Document"));
        assert_eq!(t.births["toy:Element+"].protos, ["toy:Element.prototype", "toy:P.prototype"]);
        assert_eq!(t.tags["toy:html"].map["p"], "toy:P.prototype");
        let by_id = t.entry("toy:Document.prototype.getElementById").unwrap();
        let RealmEffect::Returns(RealmOperand::Union(u)) = &by_id.effects[1] else { panic!() };
        let RealmOperand::Slot(s) = &u[1] else { panic!() };
        assert_eq!(s.steps[0], RealmSlotStep::Is(Cow::Borrowed("toy:Element.prototype")));
    }

    #[test]
    fn each_violation_is_an_error() {
        use serde_json::json;
        err(|v| v["format"] = json!("host-surface-v0"), "format");
        err(|v| v["provider"] = json!("a:b"), "provider name");
        err(|v| v["targets"] = json!([]), "targets");
        err(|v| v["extends"] = json!([]), "without `extends`");
        err(|v| v["objects"][1]["complete"] = json!(false), "not complete");
        err(|v| v["realm"]["complete"] = json!(false), "realm.global is not complete");
        err(|v| v["objects"][1]["parent"] = json!("Nowhere.prototype"), "names no node");
        err(|v| v["objects"][1]["parent"] = json!("perry-realm:Nowhere"), "not a node of the static realm");
        err(|v| v["objects"][0]["members"][1]["key"] = json!("appendChild"), "declared twice");
        err(|v| v["globals"][0]["key"] = json!("Object"), "already declared");
        err(|v| v["objects"][0]["members"][0]["effects"] = json!([]), "declares no effects");
        err(|v| v["objects"][0]["members"][0]["entry"] = json!({"symbol": "x"}), "binding kind");
        err(
            |v| v["objects"][0]["members"][0]["effects"][0]["stores"]["dst"]["root"]["pool"] = json!("nope"),
            "pool `nope`",
        );
        err(
            |v| v["objects"][3]["members"][1]["effects"][1]["returns"]["fresh_by_tag"]["tags"] = json!("svg"),
            "tag relation",
        );
        err(
            |v| v["objects"][3]["members"][1]["effects"][1]["returns"]["fresh_by_tag"]["arg"] = json!(3),
            "argument 3",
        );
        err(
            |v| v["objects"][3]["members"][0]["effects"][1]["returns"]["union"][1]["slot"]["steps"][0]["is"] =
                json!("Text.prototype"),
            "names no node",
        );
        err(
            |v| v["objects"][0]["members"][0]["effects"] = json!(["open", {"returns": {"prim": ["undefined"]}}]),
            "`open` cannot be combined",
        );
        err(
            |v| v["objects"][0]["members"][0]["effects"][1] = json!({"returns": "unknown"}),
            "`unknown` value",
        );
        err(|v| v["births"][1]["interfaces"] = json!(["Nope"]), "no prototype object");
        err(|v| v["realm"]["global"] = json!("Nope"), "names no interface");
        err(|v| v["objects"][0]["members"][0]["throws"] = json!(["perry-realm:NoError"]), "static realm");
    }

    #[test]
    fn foreign_keys_need_extends_and_never_conflict() {
        let other = r#"{"format": "host-surface-v1", "provider": "ext", "provider_id": 2, "targets": ["*"],
          "binding": {"abi": "perry-extern-c", "kind": "symbol"},
          "objects": [{"name": "toy:Node.prototype", "members": [
            {"key": "extra", "kind": "method", "attributes": 7, "entry": {"symbol": "ext_extra"},
             "effects": [{"returns": {"prim": ["undefined"]}}]}]}]}"#;
        let e = load_json(&[TOY, other]).expect_err("no extends");
        assert!(e.contains("without an `extends`"), "{e}");
        let ok = other.replace(
            r#""targets": ["*"],"#,
            r#""targets": ["*"], "extends": [{"provider": "toy", "objects": ["Node.prototype"]}],"#,
        );
        let t = load_json(&[TOY, &ok]).unwrap();
        assert!(t.entry("toy:Node.prototype").unwrap().own.iter().any(|o| o.key == "extra"));
        let clash = ok.replace("\"extra\"", "\"appendChild\"");
        let same_id = ok.replace("\"provider_id\": 2", "\"provider_id\": 1");
        let e = load_json(&[TOY, &same_id]).expect_err("same id");
        assert!(e.contains("reuses provider_id"), "{e}");
        assert!(load_json(&[TOY, &clash]).is_err());
        let e = load_json(&[TOY, TOY]).expect_err("twice");
        assert!(e.contains("loaded twice"), "{e}");
    }

    #[test]
    fn the_guard_refuses_a_host_without_a_global_surface() {
        let req = [HostRequirement { by: "package `dom-host`".into() }];
        assert!(check_required(&[], &[]).is_ok());
        let e = check_required(&req, &[]).expect_err("no surface");
        assert!(e.contains("package `dom-host` adds globals"), "{e}");
        let toy = parse(TOY, "t").unwrap();
        assert!(check_required(&req, &[toy.clone()]).is_ok());
        let mut no_global = toy;
        no_global.extends.clear();
        assert!(check_required(&req, &[no_global]).is_err());
    }
}
