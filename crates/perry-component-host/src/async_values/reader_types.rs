//! WAT type graph for one canonical async payload. Imports contain resource
//! types only; composition connects them to the original nominal identities.
use anyhow::{bail, ensure, Context, Result};
use perry_wit::{Type, World};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Resources = BTreeMap<String, (String, String)>;

pub(super) fn quote(value: &str) -> String {
    let mut text = String::from("\"");
    for byte in value.bytes() {
        use std::fmt::Write;
        write!(text, "\\{byte:02x}").unwrap();
    }
    text.push('"');
    text
}

pub(super) struct Types<'a> {
    world: &'a World,
    resources: &'a Resources,
    definitions: BTreeMap<String, String>,
    active: BTreeSet<String>,
    imports: BTreeMap<String, BTreeMap<String, String>>,
    nominal: BTreeMap<(String, String), String>,
    next: u32,
    pub text: String,
}

impl<'a> Types<'a> {
    pub fn new(world: &'a World, resources: &'a Resources) -> Self {
        Self {
            world,
            resources,
            definitions: BTreeMap::new(),
            active: BTreeSet::new(),
            imports: BTreeMap::new(),
            nominal: BTreeMap::new(),
            next: 0,
            text: String::new(),
        }
    }

    pub fn resource(&mut self, key: &str, depth: usize) -> Result<String> {
        ensure!(depth < 128, "async resource nesting exceeds 128");
        let definition = self
            .world
            .types
            .get(key)
            .context("unknown async resource")?;
        if let Type::Ref(key) = &definition.ty {
            return self.resource(key, depth + 1);
        }
        ensure!(
            matches!(definition.ty, Type::Resource),
            "expected an async resource type"
        );
        let (interface, name) = self
            .resources
            .get(key)
            .with_context(|| format!("missing original resource identity for {key}"))?;
        let binding = (interface.clone(), name.clone());
        if let Some(previous) = self.nominal.insert(binding, key.into()) {
            ensure!(
                previous == key,
                "distinct async resources cannot share one original identity"
            );
        }
        let types = self.imports.entry(interface.clone()).or_default();
        if let Some(id) = types.get(name) {
            return Ok(id.clone());
        }
        let id = format!("$r{}", self.next);
        self.next += 1;
        types.insert(name.clone(), id.clone());
        Ok(id)
    }

    pub fn imports(&self) -> String {
        let mut text = String::new();
        for (i, (interface, types)) in self.imports.iter().enumerate() {
            if interface.is_empty() {
                for (name, id) in types {
                    text += &format!("(import {} (type {id} (sub resource)))\n", quote(name));
                }
            } else {
                text += &format!("(import {} (instance $resources{i}", quote(interface));
                for name in types.keys() {
                    text += &format!(" (export {} (type (sub resource)))", quote(name));
                }
                text += "))\n";
                for (name, id) in types {
                    text += &format!("(alias export $resources{i} {} (type {id}))\n", quote(name));
                }
            }
        }
        text
    }

    pub fn value(&mut self, ty: &Type, depth: usize) -> Result<String> {
        ensure!(depth < 128, "async type nesting exceeds 128");
        if let Type::Ref(key) = ty {
            if let Some(id) = self.definitions.get(key) {
                return Ok(id.clone());
            }
            ensure!(self.active.insert(key.clone()), "cyclic async type {key}");
            let definition = self.world.types.get(key).context("unknown async type")?;
            let value = self.value(&definition.ty, depth + 1)?;
            self.active.remove(key);
            self.definitions.insert(key.clone(), value.clone());
            return Ok(value);
        }
        let primitive = match ty {
            Type::Bool => Some("bool"),
            Type::U8 => Some("u8"),
            Type::U16 => Some("u16"),
            Type::U32 => Some("u32"),
            Type::U64 => Some("u64"),
            Type::S8 => Some("s8"),
            Type::S16 => Some("s16"),
            Type::S32 => Some("s32"),
            Type::S64 => Some("s64"),
            Type::F32 => Some("float32"),
            Type::F64 => Some("float64"),
            Type::Char => Some("char"),
            Type::String => Some("string"),
            _ => None,
        };
        if let Some(primitive) = primitive {
            return Ok(primitive.into());
        }
        let expression = match ty {
            Type::Own(key) | Type::Borrow(key) => {
                let resource = self.resource(key, 0)?;
                format!(
                    "({} {resource})",
                    if matches!(ty, Type::Own(_)) {
                        "own"
                    } else {
                        "borrow"
                    }
                )
            }
            Type::List(ty) => format!("(list {})", self.value(ty, depth + 1)?),
            Type::Future(payload) | Type::Stream(payload) => {
                if matches!(ty, Type::Stream(_)) {
                    let mut item = payload.as_deref();
                    let mut count = 0;
                    while let Some(Type::Ref(key)) = item {
                        ensure!(count < 128, "async stream alias nesting exceeds 128");
                        item = Some(
                            &self
                                .world
                                .types
                                .get(key)
                                .context("unknown async stream type")?
                                .ty,
                        );
                        count += 1;
                    }
                    ensure!(!matches!(item, Some(Type::Char)),
                        "canonical stream<char> is unsupported; use stream<u8> with an explicit encoding");
                }
                let name = if matches!(ty, Type::Future(_)) {
                    "future"
                } else {
                    "stream"
                };
                let payload = payload
                    .as_ref()
                    .map(|ty| self.value(ty, depth + 1))
                    .transpose()?;
                format!(
                    "({name}{})",
                    payload.map(|p| format!(" {p}")).unwrap_or_default()
                )
            }
            Type::Record(fields) => {
                ensure!(
                    !fields.is_empty(),
                    "canonical adapter records must have at least one field"
                );
                let mut text = String::from("(record");
                for field in fields {
                    text += &format!(
                        " (field {} {})",
                        quote(&field.name),
                        self.value(&field.ty, depth + 1)?
                    );
                }
                text + ")"
            }
            Type::Tuple(fields) => {
                ensure!(
                    !fields.is_empty(),
                    "canonical adapter tuples must have at least one field"
                );
                let mut text = String::from("(tuple");
                for field in fields {
                    text += &format!(" {}", self.value(field, depth + 1)?);
                }
                text + ")"
            }
            Type::Flags(names) | Type::Enum(names) => {
                let mut text = format!(
                    "({}",
                    if matches!(ty, Type::Flags(_)) {
                        "flags"
                    } else {
                        "enum"
                    }
                );
                for name in names {
                    text += &format!(" {}", quote(name));
                }
                text + ")"
            }
            Type::Variant(cases) => {
                let mut text = String::from("(variant");
                for case in cases {
                    let payload = case
                        .ty
                        .as_ref()
                        .map(|ty| self.value(ty, depth + 1))
                        .transpose()?;
                    text += &format!(
                        " (case {}{})",
                        quote(&case.name),
                        payload.map(|p| format!(" {p}")).unwrap_or_default()
                    );
                }
                text + ")"
            }
            Type::Option(ty) => format!("(option {})", self.value(ty, depth + 1)?),
            Type::Result { ok, err } => {
                let ok = ok
                    .as_ref()
                    .map(|ty| self.value(ty, depth + 1))
                    .transpose()?;
                let err = err
                    .as_ref()
                    .map(|ty| self.value(ty, depth + 1))
                    .transpose()?;
                format!(
                    "(result {}{})",
                    ok.unwrap_or_default(),
                    err.map(|p| format!(" (error {p})")).unwrap_or_default()
                )
            }
            _ => bail!("unsupported async adapter value"),
        };
        let id = self.next;
        self.next += 1;
        // Refer to the exported alias so nominal record/variant types remain
        // visible when the whole helper instance is exported by composition.
        self.text +=
            &format!("(type $t{id} {expression})\n(export $v{id} \"t{id}\" (type $t{id}))\n");
        Ok(format!("$v{id}"))
    }
}
