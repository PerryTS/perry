use super::*;
use perry_hir::Expr;

impl<'a> Gen<'a> {
    pub(crate) fn note_indexed_allocation_sites(&mut self) {
        for mi in 0..self.modules.len() {
            let mut sites = Vec::new();
            let mut ordinal = 0;
            self.walked(mi).each_expr(&mut |e| {
                if matches!(e, Expr::TypedArrayNew { .. } | Expr::Uint8ArrayNew(_)) {
                    sites.push(((mi as u32, e as *const Expr as usize), ordinal));
                    ordinal += 1;
                }
            });
            self.indexed_allocation_sites.extend(sites);
        }
    }

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
            // Importing a new contract may create more stand-ins, so a
            // generation counter cannot identify a published source allocation.
            let Some(site) = self
                .indexed_allocation_sites
                .get(&(mi, e as *const Expr as usize))
            else {
                return false;
            };
            self.m.tmpls[*tm as usize].display = format!(
                "{}:indexed-owner@{}{}",
                self.mods[mi as usize].path_short(),
                site,
                self.unit_suffix()
            );
            self.m.ta_elem.insert(*tm, kind);
            self.m.ta_min_len.insert(*tm, len.unwrap_or(0));
            if let Some(len) = len {
                self.m.ta_exact_len.insert(*tm, len);
            }
        }
        owner.is_some()
    }
}
