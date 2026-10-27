use super::*;
use perry_hir::Expr;

impl<'a> Gen<'a> {
    /// Start an allocation contract. The solved argument node establishes
    /// numeric construction; source/view overloads cannot become fresh owners.
    pub(crate) fn begin_indexed_owner(&mut self, mi: u32, t: Tok, e: &Expr) -> bool {
        let owner = match e {
            Expr::TypedArrayNew {
                kind,
                arg: Some(arg),
            } => Some((*kind, self.owner_extent(mi, arg))),
            Expr::Uint8ArrayNew(Some(len)) => Some((1, self.owner_extent(mi, len))),
            _ => None,
        };
        // Source/view constructors are not owner proofs merely because
        // their annotation names an element width.
        if let (Some((kind, len)), TokKind::Obj(tm, _)) = (owner, &self.m.toks[t as usize]) {
            // bobj's ordinal is generation-local. The declaration
            // unit is part of a published allocation's identity.
            let unit_suffix = self.unit_suffix().to_owned();
            self.m.tmpls[*tm as usize].display.push_str(&unit_suffix);
            self.m.ta_elem.insert(*tm, kind);
            self.m.ta_min_len.insert(*tm, len.unwrap_or(0));
            if let Some(len) = len {
                self.m.ta_exact_len.insert(*tm, len);
            }
        }
        owner.is_some()
    }
}
