mod discard;
use super::{literal, Names, Resources};
use anyhow::{bail, Result};
use std::fmt::Write;
use wit_parser::{Resolve, Type, TypeDefKind};

pub(super) struct Emitter<'a> {
    resolve: &'a Resolve,
    names: &'a Names,
    resources: &'a Resources,
    world: &'a str,
    serial: usize,
    pub source: String,
}

impl<'a> Emitter<'a> {
    pub fn new(
        resolve: &'a Resolve,
        names: &'a Names,
        resources: &'a Resources,
        world: &'a str,
    ) -> Self {
        Self {
            resolve,
            names,
            resources,
            world,
            serial: 0,
            source: String::new(),
        }
    }
    fn name(&mut self) -> String {
        let name = format!("v{}", self.serial);
        self.serial += 1;
        name
    }
    pub fn slot(&mut self, expression: &str) -> String {
        let name = self.name();
        writeln!(self.source, "uint32_t {name} = {expression};").unwrap();
        name
    }
    pub fn ctype(&self, ty: Type) -> String {
        match ty {
            Type::Bool => "bool".into(),
            Type::Char | Type::U32 => "uint32_t".into(),
            Type::U8 => "uint8_t".into(),
            Type::U16 => "uint16_t".into(),
            Type::U64 => "uint64_t".into(),
            Type::S8 => "int8_t".into(),
            Type::S16 => "int16_t".into(),
            Type::S32 => "int32_t".into(),
            Type::S64 => "int64_t".into(),
            Type::F32 => "float".into(),
            Type::F64 => "double".into(),
            Type::String => format!("{}_string_t", self.world),
            Type::Id(id) => self.names[&id].clone(),
            Type::ErrorContext => unreachable!("P3 type rejected before generation"),
        }
    }
    fn byte(&self, ty: Type) -> bool {
        match ty {
            Type::U8 => true,
            Type::Id(id) => match self.resolve.types[id].kind {
                TypeDefKind::Type(ty) => self.byte(ty),
                _ => false,
            },
            _ => false,
        }
    }
    fn owns_memory(&self, ty: Type) -> bool {
        match ty {
            Type::String => true,
            Type::Id(id) => match &self.resolve.types[id].kind {
                TypeDefKind::Type(ty) | TypeDefKind::Option(ty) => self.owns_memory(*ty),
                TypeDefKind::List(_) => true,
                TypeDefKind::Record(record) => record.fields.iter().any(|f| self.owns_memory(f.ty)),
                TypeDefKind::Tuple(tuple) => tuple.types.iter().any(|ty| self.owns_memory(*ty)),
                TypeDefKind::Variant(variant) => variant
                    .cases
                    .iter()
                    .any(|c| c.ty.is_some_and(|ty| self.owns_memory(ty))),
                TypeDefKind::Result(result) => result
                    .ok
                    .into_iter()
                    .chain(result.err)
                    .any(|ty| self.owns_memory(ty)),
                _ => false,
            },
            _ => false,
        }
    }
    pub fn free(&mut self, ty: Type, pointer: &str) {
        if self.owns_memory(ty) {
            let name = self.ctype(ty);
            writeln!(
                self.source,
                "{}_free({pointer});",
                name.strip_suffix("_t").unwrap()
            )
            .unwrap();
        }
    }
    fn text(&mut self, text: &str) -> String {
        self.slot(&format!(
            "perry_component_frame_string(f, (const uint8_t *){}, {})",
            literal(text),
            text.len()
        ))
    }
    fn set(&mut self, object: &str, key: &str, value: &str) {
        writeln!(
            self.source,
            "perry_component_frame_set(f, {object}, (const uint8_t *){}, {}, {value});",
            literal(key),
            key.len()
        )
        .unwrap();
    }
    fn get(&mut self, object: &str, key: &str) -> String {
        self.slot(&format!(
            "perry_component_frame_get(f, {object}, (const uint8_t *){}, {})",
            literal(key),
            key.len()
        ))
    }
    fn tag(&self, slot: &str, text: &str) -> String {
        format!("perry_tag(f, {slot}, {}, {})", literal(text), text.len())
    }
    fn tagged_lift(
        &mut self,
        output: &str,
        tag: &str,
        payload: Option<Type>,
        value: &str,
    ) -> Result<()> {
        let label = self.text(tag);
        self.set(output, "tag", &label);
        if let Some(ty) = payload {
            let child = self.lift(ty, value)?;
            self.set(output, "value", &child);
        }
        Ok(())
    }

    /// Lift an owned/borrowed canonical C value into rooted JS frame slots.
    /// The caller frees owned C inputs after this has copied their contents.
    pub fn lift(&mut self, ty: Type, input: &str) -> Result<String> {
        let input = format!("({input})");
        let primitive = match ty {
            Type::Bool => Some(format!("perry_component_frame_bool(f, {input})")),
            Type::U64 => Some(format!("perry_component_frame_u64(f, {input})")),
            Type::S64 => Some(format!("perry_component_frame_s64(f, {input})")),
            Type::Char => Some(format!("perry_component_frame_char(f, {input})")),
            Type::String => Some(format!(
                "perry_component_frame_string(f, {input}.ptr, {input}.len)"
            )),
            Type::Id(_) => None,
            _ => Some(format!("perry_component_frame_number(f, {input})")),
        };
        if let Some(expression) = primitive {
            return Ok(self.slot(&expression));
        }
        let Type::Id(id) = ty else { unreachable!() };
        let resolve = self.resolve;
        match &resolve.types[id].kind {
            TypeDefKind::Handle(handle) => {
                let (resource, owned) = match handle {
                    wit_parser::Handle::Own(id) => (*id, true),
                    wit_parser::Handle::Borrow(id) => (*id, false),
                };
                let resource = crate::resolve::resource_id(self.resolve, resource);
                let info = &self.resources[&resource];
                let handle = if info.local && !owned {
                    "0".into()
                } else {
                    format!("{input}.__handle")
                };
                let rep = if !info.local {
                    "0".into()
                } else if owned {
                    format!(
                        "(uint32_t)(uintptr_t){}_rep({input})",
                        info.name.strip_suffix("_t").unwrap()
                    )
                } else {
                    format!("(uint32_t)(uintptr_t){input}")
                };
                Ok(self.slot(&format!(
                    "perry_component_frame_resource_lift(f, {}, (uint32_t){handle}, {rep}, {})",
                    info.id,
                    u8::from(owned)
                )))
            }
            TypeDefKind::Type(ty) => self.lift(*ty, &input),
            TypeDefKind::Record(record) => {
                let output = self.slot("perry_component_frame_object(f)");
                for field in &record.fields {
                    let child = self.lift(
                        field.ty,
                        &format!("{input}.{}", wit_bindgen_c::to_c_ident(&field.name)),
                    )?;
                    self.set(&output, &field.name, &child);
                }
                Ok(output)
            }
            TypeDefKind::Tuple(tuple) => {
                let output = self.slot(&format!(
                    "perry_component_frame_array(f, {})",
                    tuple.types.len()
                ));
                for (i, ty) in tuple.types.iter().enumerate() {
                    let child = self.lift(*ty, &format!("{input}.f{i}"))?;
                    writeln!(
                        self.source,
                        "perry_component_frame_array_set(f, {output}, {i}, {child});"
                    )?;
                }
                Ok(output)
            }
            TypeDefKind::List(ty) if self.byte(*ty) => Ok(self.slot(&format!(
                "perry_component_frame_bytes(f, {input}.ptr, {input}.len)"
            ))),
            TypeDefKind::List(ty) => {
                writeln!(
                    self.source,
                    "if ({input}.len > UINT32_MAX) perry_bad(f, \"Component list is too long\");"
                )?;
                let output = self.slot(&format!(
                    "perry_component_frame_array(f, (uint32_t){input}.len)"
                ));
                let i = self.name();
                writeln!(self.source, "for (size_t {i} = 0; {i} < {input}.len && !perry_component_frame_failed(f); ++{i}) {{")?;
                let child = self.lift(*ty, &format!("{input}.ptr[{i}]"))?;
                writeln!(
                    self.source,
                    "perry_component_frame_array_set(f, {output}, (uint32_t){i}, {child});\n}}"
                )?;
                Ok(output)
            }
            TypeDefKind::Enum(enumeration) => {
                let output = self.slot("0");
                writeln!(self.source, "switch ({input}) {{")?;
                for (i, case) in enumeration.cases.iter().enumerate() {
                    writeln!(self.source, "case {i}: {{")?;
                    let value = self.text(&case.name);
                    writeln!(self.source, "{output} = {value}; break; }}")?;
                }
                self.source
                    .push_str("default: perry_bad(f, \"Invalid canonical enum\"); break;\n}\n");
                Ok(output)
            }
            TypeDefKind::Flags(flags) => {
                // Frame array writes are bounded; they do not grow an empty
                // array. The binary profile admits at most 32 flag labels.
                let output = self.slot(&format!(
                    "perry_component_frame_array(f, (uint32_t)__builtin_popcount((uint32_t){input}))"
                ));
                let index = self.slot("0");
                for (i, flag) in flags.flags.iter().enumerate() {
                    writeln!(
                        self.source,
                        "if ((uint64_t){input} & (UINT64_C(1) << {i})) {{"
                    )?;
                    let value = self.text(&flag.name);
                    writeln!(
                        self.source,
                        "perry_component_frame_array_set(f, {output}, {index}++, {value});\n}}"
                    )?;
                }
                Ok(output)
            }
            TypeDefKind::Variant(variant) => {
                let output = self.slot("perry_component_frame_object(f)");
                writeln!(self.source, "switch ({input}.tag) {{")?;
                for (i, case) in variant.cases.iter().enumerate() {
                    writeln!(self.source, "case {i}: {{")?;
                    self.tagged_lift(
                        &output,
                        &case.name,
                        case.ty,
                        &format!("{input}.val.{}", wit_bindgen_c::to_c_ident(&case.name)),
                    )?;
                    self.source.push_str("break; }\n");
                }
                self.source
                    .push_str("default: perry_bad(f, \"Invalid canonical variant\"); break;\n}\n");
                Ok(output)
            }
            TypeDefKind::Option(ty) => {
                let output = self.slot("perry_component_frame_object(f)");
                writeln!(self.source, "if ({input}.is_some) {{")?;
                self.tagged_lift(&output, "some", Some(*ty), &format!("{input}.val"))?;
                self.source.push_str("} else {\n");
                self.tagged_lift(&output, "none", None, "")?;
                self.source.push_str("}\n");
                Ok(output)
            }
            TypeDefKind::Result(result) => {
                let output = self.slot("perry_component_frame_object(f)");
                writeln!(self.source, "if ({input}.is_err) {{")?;
                self.tagged_lift(&output, "err", result.err, &format!("{input}.val.err"))?;
                self.source.push_str("} else {\n");
                self.tagged_lift(&output, "ok", result.ok, &format!("{input}.val.ok"))?;
                self.source.push_str("}\n");
                Ok(output)
            }
            _ => bail!("unsupported canonical guest type"),
        }
    }

    /// All allocations made while lowering are tracked until ownership is
    /// transferred, so a late field/type error cannot leak earlier buffers.
    pub fn lower(&mut self, ty: Type, slot: &str, destination: &str) -> Result<()> {
        self.source
            .push_str("if (!perry_component_frame_failed(f)) {\n");
        self.lower_inner(ty, slot, &format!("({destination})"))?;
        self.source.push_str("}\n");
        Ok(())
    }
    fn lower_inner(&mut self, ty: Type, slot: &str, dst: &str) -> Result<()> {
        let primitive = match ty {
            Type::Bool => Some(format!("perry_component_frame_get_bool(f, {slot})")),
            Type::U64 => Some(format!("perry_component_frame_get_u64(f, {slot})")),
            Type::S64 => Some(format!("perry_component_frame_get_s64(f, {slot})")),
            Type::Char => Some(format!("perry_component_frame_get_char(f, {slot})")),
            Type::F32 | Type::F64 => Some(format!("perry_component_frame_get_number(f, {slot})")),
            Type::U8 | Type::U16 | Type::U32 | Type::S8 | Type::S16 | Type::S32 => {
                let (min, max) = match ty {
                    Type::U8 => ("0", "255"),
                    Type::U16 => ("0", "65535"),
                    Type::U32 => ("0", "4294967295"),
                    Type::S8 => ("-128", "127"),
                    Type::S16 => ("-32768", "32767"),
                    _ => ("-2147483648", "2147483647"),
                };
                Some(format!(
                    "perry_component_frame_get_integer(f, {slot}, {min}, {max})"
                ))
            }
            _ => None,
        };
        if let Some(expression) = primitive {
            writeln!(self.source, "{dst} = ({}){expression};", self.ctype(ty))?;
            return Ok(());
        }
        if ty == Type::String {
            writeln!(self.source, "{dst}.len = perry_component_frame_string_len(f, {slot});\n{dst}.ptr = perry_allocate(f, &arena, {dst}.len, 1);\nif (!perry_component_frame_failed(f)) perry_component_frame_string_copy(f, {slot}, {dst}.ptr, {dst}.len);")?;
            return Ok(());
        }
        let Type::Id(id) = ty else {
            bail!("unsupported canonical guest type")
        };
        let resolve = self.resolve;
        match &resolve.types[id].kind {
            TypeDefKind::Handle(handle) => {
                let (resource, owned) = match handle {
                    wit_parser::Handle::Own(id) => (*id, true),
                    wit_parser::Handle::Borrow(id) => (*id, false),
                };
                let resource = crate::resolve::resource_id(self.resolve, resource);
                let info = &self.resources[&resource];
                let local = info.local;
                let value = format!(
                    "perry_component_frame_resource_lower(f, {slot}, {}, {}, {})",
                    info.id,
                    u8::from(owned),
                    u8::from(local)
                );
                if local && !owned {
                    writeln!(
                        self.source,
                        "{dst} = ({}) (uintptr_t){value};",
                        self.ctype(ty)
                    )?;
                } else {
                    writeln!(self.source, "{dst}.__handle = (int32_t){value};")?;
                }
            }
            TypeDefKind::Type(ty) => self.lower(*ty, slot, dst)?,
            TypeDefKind::Record(record) => {
                for field in &record.fields {
                    let value = self.get(slot, &field.name);
                    self.lower(
                        field.ty,
                        &value,
                        &format!("{dst}.{}", wit_bindgen_c::to_c_ident(&field.name)),
                    )?;
                }
            }
            TypeDefKind::Tuple(tuple) => {
                writeln!(self.source, "if (perry_component_frame_array_len(f, {slot}) != {}) perry_bad(f, \"WIT tuple length mismatch\");", tuple.types.len())?;
                for (i, ty) in tuple.types.iter().enumerate() {
                    let value =
                        self.slot(&format!("perry_component_frame_array_get(f, {slot}, {i})"));
                    self.lower(*ty, &value, &format!("{dst}.f{i}"))?;
                }
            }
            TypeDefKind::List(ty) => {
                let byte = self.byte(*ty);
                let length = if byte { "bytes" } else { "array" };
                writeln!(self.source, "{dst}.len = perry_component_frame_{length}_len(f, {slot});\n{dst}.ptr = perry_allocate(f, &arena, {dst}.len, sizeof(*{dst}.ptr));")?;
                if byte {
                    writeln!(self.source, "if (!perry_component_frame_failed(f)) perry_component_frame_bytes_copy(f, {slot}, {dst}.ptr, {dst}.len);")?;
                } else {
                    let i = self.name();
                    writeln!(self.source, "for (size_t {i} = 0; {i} < {dst}.len && !perry_component_frame_failed(f); ++{i}) {{")?;
                    let value = self.slot(&format!(
                        "perry_component_frame_array_get(f, {slot}, (uint32_t){i})"
                    ));
                    self.lower(*ty, &value, &format!("{dst}.ptr[{i}]"))?;
                    self.source.push_str("}\n");
                }
            }
            TypeDefKind::Enum(enumeration) => {
                for (i, case) in enumeration.cases.iter().enumerate() {
                    writeln!(
                        self.source,
                        "{}if ({}) {{ {dst} = {i}; }}",
                        if i == 0 { "" } else { "else " },
                        self.tag(slot, &case.name)
                    )?;
                }
                self.source
                    .push_str("else perry_bad(f, \"Unknown WIT enum case\");\n");
            }
            TypeDefKind::Flags(flags) => {
                let len = self.slot(&format!("perry_component_frame_array_len(f, {slot})"));
                let i = self.name();
                writeln!(self.source, "{dst} = 0;\nfor (uint32_t {i} = 0; {i} < {len} && !perry_component_frame_failed(f); ++{i}) {{")?;
                let value = self.slot(&format!("perry_component_frame_array_get(f, {slot}, {i})"));
                for (n, flag) in flags.flags.iter().enumerate() {
                    writeln!(self.source, "{}if ({}) {{\nif ((uint64_t){dst} & (UINT64_C(1) << {n})) perry_bad(f, \"Duplicate WIT flag\");\n{dst} |= UINT64_C(1) << {n};\n}}", if n == 0 { "" } else { "else " }, self.tag(&value, &flag.name))?;
                }
                if !flags.flags.is_empty() {
                    self.source.push_str("else ");
                }
                self.source
                    .push_str("perry_bad(f, \"Unknown WIT flag\");\n}\n");
            }
            TypeDefKind::Variant(variant) => {
                let tag = self.get(slot, "tag");
                for (i, case) in variant.cases.iter().enumerate() {
                    writeln!(
                        self.source,
                        "{}if ({}) {{\n{dst}.tag = {i};",
                        if i == 0 { "" } else { "else " },
                        self.tag(&tag, &case.name)
                    )?;
                    if let Some(ty) = case.ty {
                        let value = self.get(slot, "value");
                        self.lower(
                            ty,
                            &value,
                            &format!("{dst}.val.{}", wit_bindgen_c::to_c_ident(&case.name)),
                        )?;
                    }
                    self.source.push_str("}\n");
                }
                self.source
                    .push_str("else perry_bad(f, \"Unknown WIT variant case\");\n");
            }
            TypeDefKind::Option(ty) => {
                let tag = self.get(slot, "tag");
                writeln!(
                    self.source,
                    "if ({}) {{\n{dst}.is_some = true;",
                    self.tag(&tag, "some")
                )?;
                let value = self.get(slot, "value");
                self.lower(*ty, &value, &format!("{dst}.val"))?;
                writeln!(self.source, "}} else if ({}) {{ {dst}.is_some = false; }}\nelse perry_bad(f, \"Expected a tagged WIT option\");", self.tag(&tag, "none"))?;
            }
            TypeDefKind::Result(result) => {
                let tag = self.get(slot, "tag");
                for (i, (tag_name, ty)) in [("ok", result.ok), ("err", result.err)]
                    .into_iter()
                    .enumerate()
                {
                    writeln!(
                        self.source,
                        "{}if ({}) {{\n{dst}.is_err = {};",
                        if i == 0 { "" } else { "else " },
                        self.tag(&tag, tag_name),
                        i == 1
                    )?;
                    if let Some(ty) = ty {
                        let value = self.get(slot, "value");
                        self.lower(ty, &value, &format!("{dst}.val.{tag_name}"))?;
                    }
                    self.source.push_str("}\n");
                }
                self.source
                    .push_str("else perry_bad(f, \"Expected a tagged WIT result\");\n");
            }
            _ => bail!("unsupported canonical guest type"),
        }
        Ok(())
    }
}
