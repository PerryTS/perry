//! Borrowed canonical input handles remain in the forwarding instance after
//! the original synchronous call. Retire them before freeing buffers or
//! starting the private actor. Owned inputs have already transferred.
use super::*;

impl Emitter<'_> {
    pub fn has_borrows(&self, ty: Type) -> bool {
        let Type::Id(id) = ty else { return false };
        match &self.resolve.types[id].kind {
            TypeDefKind::Handle(wit_parser::Handle::Borrow(_)) => true,
            TypeDefKind::Type(ty) | TypeDefKind::Option(ty) | TypeDefKind::List(ty) => {
                self.has_borrows(*ty)
            }
            TypeDefKind::Record(record) => record.fields.iter().any(|f| self.has_borrows(f.ty)),
            TypeDefKind::Tuple(tuple) => tuple.types.iter().any(|ty| self.has_borrows(*ty)),
            TypeDefKind::Variant(variant) => variant
                .cases
                .iter()
                .any(|c| c.ty.is_some_and(|ty| self.has_borrows(ty))),
            TypeDefKind::Result(result) => result
                .ok
                .into_iter()
                .chain(result.err)
                .any(|ty| self.has_borrows(ty)),
            _ => false,
        }
    }

    /// The forwarded call has returned, so only this instance's original
    /// input borrow handles remain. Retire each without dropping any owner.
    pub fn retire_borrows(&mut self, ty: Type, input: &str) {
        if !self.has_borrows(ty) {
            return;
        }
        let input = format!("({input})");
        let Type::Id(id) = ty else { unreachable!() };
        let resolve = self.resolve;
        match &resolve.types[id].kind {
            TypeDefKind::Handle(wit_parser::Handle::Borrow(resource)) => {
                let resource = crate::resolve::resource_id(resolve, *resource);
                let stem = self.resources[&resource].name.strip_suffix("_t").unwrap();
                writeln!(self.source, "{stem}_drop_borrow({input});").unwrap();
            }
            TypeDefKind::Type(ty) => self.retire_borrows(*ty, &input),
            TypeDefKind::Record(record) => {
                for field in &record.fields {
                    self.retire_borrows(
                        field.ty,
                        &format!("{input}.{}", wit_bindgen_c::to_c_ident(&field.name)),
                    );
                }
            }
            TypeDefKind::Tuple(tuple) => {
                for (i, ty) in tuple.types.iter().enumerate() {
                    self.retire_borrows(*ty, &format!("{input}.f{i}"));
                }
            }
            TypeDefKind::List(ty) => {
                let i = self.name();
                writeln!(
                    self.source,
                    "for (size_t {i} = 0; {i} < {input}.len; ++{i}) {{"
                )
                .unwrap();
                self.retire_borrows(*ty, &format!("{input}.ptr[{i}]"));
                self.source.push_str("}\n");
            }
            TypeDefKind::Variant(variant) => {
                writeln!(self.source, "switch ({input}.tag) {{").unwrap();
                for (i, case) in variant.cases.iter().enumerate() {
                    if let Some(ty) = case.ty.filter(|ty| self.has_borrows(*ty)) {
                        writeln!(self.source, "case {i}: {{").unwrap();
                        self.retire_borrows(
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
                self.retire_borrows(*ty, &format!("{input}.val"));
                self.source.push_str("}\n");
            }
            TypeDefKind::Result(result) => {
                writeln!(self.source, "if ({input}.is_err) {{").unwrap();
                if let Some(ty) = result.err {
                    self.retire_borrows(ty, &format!("{input}.val.err"));
                }
                self.source.push_str("} else {\n");
                if let Some(ty) = result.ok {
                    self.retire_borrows(ty, &format!("{input}.val.ok"));
                }
                self.source.push_str("}\n");
            }
            _ => unreachable!("has_borrows only admits compound values and borrowed handles"),
        }
    }
}
