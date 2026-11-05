//! Canonical owners which were committed but never handed to the other side.
//! Buffer cleanup stays separate: the maintained C backend's `_free` helpers
//! deliberately preserve resource handles during an ordinary JS lift.
use super::*;

impl Emitter<'_> {
    pub fn owns_resources(&self, ty: Type) -> bool {
        let Type::Id(id) = ty else { return false };
        match &self.resolve.types[id].kind {
            TypeDefKind::Handle(wit_parser::Handle::Own(_)) => true,
            TypeDefKind::Type(ty) | TypeDefKind::Option(ty) | TypeDefKind::List(ty) => {
                self.owns_resources(*ty)
            }
            TypeDefKind::Record(record) => record.fields.iter().any(|f| self.owns_resources(f.ty)),
            TypeDefKind::Tuple(tuple) => tuple.types.iter().any(|ty| self.owns_resources(*ty)),
            TypeDefKind::Variant(variant) => variant
                .cases
                .iter()
                .any(|c| c.ty.is_some_and(|ty| self.owns_resources(ty))),
            TypeDefKind::Result(result) => result
                .ok
                .into_iter()
                .chain(result.err)
                .any(|ty| self.owns_resources(ty)),
            _ => false,
        }
    }

    /// Only call on a fully initialized value with committed ownership, after
    /// native acknowledgement proves that nobody else can still consume it.
    /// Borrowed handles are never dropped. Free buffers separately afterwards.
    pub fn discard_owned(&mut self, ty: Type, input: &str) {
        if !self.owns_resources(ty) {
            return;
        }
        let input = format!("({input})");
        let Type::Id(id) = ty else { unreachable!() };
        let resolve = self.resolve;
        match &resolve.types[id].kind {
            TypeDefKind::Handle(wit_parser::Handle::Own(resource)) => {
                let resource = crate::resolve::resource_id(resolve, *resource);
                let stem = self.resources[&resource].name.strip_suffix("_t").unwrap();
                writeln!(self.source, "{stem}_drop_own({input});").unwrap();
            }
            TypeDefKind::Type(ty) => self.discard_owned(*ty, &input),
            TypeDefKind::Record(record) => {
                for field in &record.fields {
                    self.discard_owned(
                        field.ty,
                        &format!("{input}.{}", wit_bindgen_c::to_c_ident(&field.name)),
                    );
                }
            }
            TypeDefKind::Tuple(tuple) => {
                for (i, ty) in tuple.types.iter().enumerate() {
                    self.discard_owned(*ty, &format!("{input}.f{i}"));
                }
            }
            TypeDefKind::List(ty) => {
                let i = self.name();
                writeln!(
                    self.source,
                    "for (size_t {i} = 0; {i} < {input}.len; ++{i}) {{"
                )
                .unwrap();
                self.discard_owned(*ty, &format!("{input}.ptr[{i}]"));
                self.source.push_str("}\n");
            }
            TypeDefKind::Variant(variant) => {
                writeln!(self.source, "switch ({input}.tag) {{").unwrap();
                for (i, case) in variant.cases.iter().enumerate() {
                    if let Some(ty) = case.ty.filter(|ty| self.owns_resources(*ty)) {
                        writeln!(self.source, "case {i}: {{").unwrap();
                        self.discard_owned(
                            ty,
                            &format!("{input}.val.{}", wit_bindgen_c::to_c_ident(&case.name)),
                        );
                        self.source.push_str("break; }\n");
                    }
                }
                self.source.push_str("default: break;\n}\n");
            }
            TypeDefKind::Option(ty) => {
                writeln!(self.source, "if ({input}.is_some) {{").unwrap();
                self.discard_owned(*ty, &format!("{input}.val"));
                self.source.push_str("}\n");
            }
            TypeDefKind::Result(result) => {
                writeln!(self.source, "if ({input}.is_err) {{").unwrap();
                if let Some(ty) = result.err {
                    self.discard_owned(ty, &format!("{input}.val.err"));
                }
                self.source.push_str("} else {\n");
                if let Some(ty) = result.ok {
                    self.discard_owned(ty, &format!("{input}.val.ok"));
                }
                self.source.push_str("}\n");
            }
            _ => unreachable!("owns_resources only admits compound values and owned handles"),
        }
    }
}
