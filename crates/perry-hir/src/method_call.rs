//! Shared admission of the method entry used by facts, the driver and codegen.
use crate::{Class, Function, Module};

/// The named receiver form understood by the proven-method lowering.
pub fn named_site(key: &str, offset: u32) -> bool {
    offset != 0 && !key.starts_with('#')
}

/// A callable entry whose ABI is the ordinary method ABI.
pub fn ordinary_entry(f: &Function) -> bool {
    !f.is_generator && !f.is_async && !f.body.iter().any(|st| {
        matches!(st, crate::Stmt::Return(Some(e)) if crate::fn_proto::is_record_generator_birth(e))
    })
}

/// The last registered method is the requested ordinary public entry.
pub fn target_in_class<'a>(c: &'a Class, fid: u32, key: &str) -> Option<&'a Function> {
    let f = c.methods.iter().rev().find(|f| f.name == key)?;
    if f.id != fid
        || !ordinary_entry(f)
        || c.has_evaluated_accessor()
        || c.static_accessor_names.iter().any(|n| n == key)
    {
        return None;
    }
    Some(f)
}

/// A holder's identity and name must each name exactly one class.
pub fn target<'a>(
    hir: &'a Module,
    cid: u32,
    fid: u32,
    key: &str,
) -> Option<(&'a Class, &'a Function)> {
    let mut classes = hir.classes.iter().filter(|c| c.id == cid);
    let c = classes.next()?;
    if classes.next().is_some()
        || hir
            .classes
            .iter()
            .filter(|other| other.name == c.name)
            .count()
            != 1
    {
        return None;
    }
    Some((c, target_in_class(c, fid, key)?))
}

/// The global console call shape handled by the console lowering.
pub fn console_method(callee: &crate::Expr) -> Option<&str> {
    use crate::Expr;
    let Expr::PropertyGet {
        object, property, ..
    } = callee
    else {
        return None;
    };
    (matches!(object.as_ref(), Expr::GlobalGet(_))
        && matches!(
            property.as_str(),
            "log"
                | "info"
                | "warn"
                | "error"
                | "debug"
                | "dir"
                | "dirxml"
                | "profile"
                | "profileEnd"
                | "timeStamp"
                | "table"
                | "trace"
                | "group"
                | "groupEnd"
                | "groupCollapsed"
                | "time"
                | "timeEnd"
                | "timeLog"
                | "count"
                | "countReset"
                | "clear"
                | "assert"
        ))
    .then_some(property.as_str())
}
