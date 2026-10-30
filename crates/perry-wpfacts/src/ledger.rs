//! The facts ledger's dump-only parts (`PERRY_FACTS_DUMP`): the closed list
//! of first-unknown causes, and the completeness census of HIR sites.
//! Nothing here adds a node, a constraint or a proof input.
use crate::pts::FxHashSet;
use perry_hir::{Expr, Module};
use std::collections::BTreeMap;

/// Why the first unknown value on a site's proof path is unknown. The list
/// is closed: a new origin gets a new named member here, never a catch-all.
/// `classify` maps every seed text of the crate to one member (the
/// `every_seed_text_has_a_cause` test scans the sources).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Unknown {
    /// A parameter of a function another unit calls (no contract for it).
    UnitEdgeParameter,
    /// The receiver of a function another unit calls.
    UnitEdgeThis,
    /// A value another unit returns, stores or publishes (result, field,
    /// shared object, broken layout of another unit).
    UnitEdgeValue,
    /// A parameter of a function unknown code calls.
    EscapedParameter,
    /// The receiver of a function unknown code calls.
    EscapedThis,
    /// The result of (or a value handed back by) an unknown callee.
    UnknownCalleeEscape,
    /// ⊤ stored into an intrinsic prototype or builtin function value: every
    /// builtin method read through it is unknown.
    IntrinsicPrototypeTopStore,
    /// A result of, or callback argument from, an opaque or unmodeled
    /// builtin.
    OpaqueBuiltin,
    /// A computed key or computed specifier.
    ComputedKey,
    /// A field of a program object unknown code can write.
    EscapedObjectField,
    /// A module namespace, re-export or live binding the analysis does not
    /// resolve.
    ModuleNamespace,
    /// A function value without a known body.
    FunctionWithoutIdentity,
    /// Host input: native modules, process, host globals.
    ExternalInput,
    /// A reflective operation took the object (defineProperty, delete,
    /// freeze, setPrototypeOf, Proxy, `with`).
    ReflectiveOperation,
    /// Interpreted code (eval / new Function / vm) can reach the value.
    DynamicCode,
    /// A HIR construct the analysis does not model.
    UnmodeledConstruct,
    /// A typed array or ArrayBuffer aliased or resized (B-codes).
    BufferAliasing,
    /// A unit's solve stopped at the work bound.
    WorkBound,
    /// ⊤ arrived with no recorded seed on its first-arrival path.
    UntracedTop,
    /// No value flows into the operand (an unexecuted or dead site).
    NoValues,
    /// The operand holds only another unit's parameter marker.
    UnitEdgeMarker,
    /// Known receivers whose layouts disagree (slots differ, class objects
    /// mixed with instances): polymorphic beyond the representable set.
    PolymorphicBeyondSet,
    /// The key is not a definite birth field of the receiver layout.
    KeyNotDefinite,
    /// An accessor holds the key (E25).
    AccessorKey,
    /// The receiver is a primitive.
    PrimitiveReceiver,
    /// The receiver is a builtin or function value, not a layout.
    BuiltinReceiver,
    /// The receiver is an object the facts do not describe as a layout.
    ReceiverNotALayout,
    /// The method value read through the receiver holds ⊤.
    TopCallee,
    /// The callee set holds a non-function, bound, class or runtime value.
    NonFunctionCallee,
    /// The receiver layout is hard (E5-E9) or its prototype chain is open.
    HardLayout,
    /// The static is not a constant of its class.
    NotAConstantStatic,
}

impl Unknown {
    pub(crate) fn name(self) -> &'static str {
        use Unknown::*;
        match self {
            UnitEdgeParameter => "unit-edge-parameter",
            UnitEdgeThis => "unit-edge-this",
            UnitEdgeValue => "unit-edge-value",
            EscapedParameter => "escaped-parameter",
            EscapedThis => "escaped-this",
            UnknownCalleeEscape => "unknown-callee-escape",
            IntrinsicPrototypeTopStore => "intrinsic-prototype-top-store",
            OpaqueBuiltin => "opaque-builtin",
            ComputedKey => "computed-key",
            EscapedObjectField => "escaped-object-field",
            ModuleNamespace => "module-namespace",
            FunctionWithoutIdentity => "function-without-identity",
            ExternalInput => "external-input",
            ReflectiveOperation => "reflective-operation",
            DynamicCode => "dynamic-code",
            UnmodeledConstruct => "unmodeled-construct",
            BufferAliasing => "buffer-aliasing",
            WorkBound => "work-bound",
            UntracedTop => "untraced-top",
            NoValues => "no-values",
            UnitEdgeMarker => "unit-edge-marker",
            PolymorphicBeyondSet => "polymorphic-beyond-set",
            KeyNotDefinite => "key-not-definite",
            AccessorKey => "accessor-key",
            PrimitiveReceiver => "primitive-receiver",
            BuiltinReceiver => "builtin-receiver",
            ReceiverNotALayout => "receiver-not-a-layout",
            TopCallee => "top-callee",
            NonFunctionCallee => "non-function-callee",
            HardLayout => "hard-layout",
            NotAConstantStatic => "not-a-constant-static",
        }
    }
}

/// How an escaped value left the analysis: the text after `: ` of a
/// composite origin, an escape reason (`Model::reason`).
#[derive(Debug)]
pub(crate) enum Route {
    Unit,
    Unknown,
    Intrinsic,
    Builtin,
    Dynamic,
    Computed,
    Reflective,
    External,
    Buffer,
}

/// `E<n> ...` / `B<n> ...`: a numbered hard-escape (design E-codes) or
/// buffer-aliasing (B-codes) reason.
fn code(w: &str, letter: char) -> bool {
    let mut c = w.chars();
    c.next() == Some(letter) && c.next().is_some_and(|d| d.is_ascii_digit())
}

pub(crate) fn route(why: &str) -> Option<Route> {
    let w = why;
    let starts = |p: &str| w.starts_with(p);
    Some(
        if w.contains("another unit")
            || w.contains("other units")
            || w.contains("unit boundary")
            || w.contains("published")
        {
            Route::Unit
        } else if w.contains("dynamic code") || w.contains("interpreted code") || starts("E14 ") {
            Route::Dynamic
        } else if starts("argument of a written ")
            || starts("stored into a builtin function value")
            || starts("store through an unknown prototype chain")
            || starts("native prototype may be written")
            || starts("E21 native parent")
        {
            Route::Intrinsic
        } else if starts("dynamic import specifier")
            || starts("module loaded by a computed specifier")
            || starts("accessor under a computed key")
        {
            Route::Computed
        } else if starts("E20 ") {
            // Stored on a builtin/global object: any code can read it.
            Route::Unknown
        } else if code(w, 'E') {
            Route::Reflective
        } else if code(w, 'B') {
            Route::Buffer
        } else if starts("node:module namespace")
            || (starts("globalThis.") && w.ends_with("unmodeled code"))
        {
            Route::External
        } else if w.contains("unknown function")
            || w.contains("unknown constructor")
            || w.contains("from unmodeled code")
            || w.contains("unknown code")
            || starts("unknown holder of the box")
            || starts("unregistered function id")
            || starts("bind with spread arguments")
            || w.contains("decorator")
        {
            Route::Unknown
        } else if starts("argument of native ")
            || starts("argument of builtin ")
            || starts("argument of a primitive method")
            || starts("argument of unmodeled runtime entry")
            || starts("argument of runtime glue")
            || starts("Array.prototype method on an array-like")
            || starts("passed to an opaque builtin")
            || w.contains("of an opaque builtin function")
            || starts("RegExp operand")
            || starts("getOwnPropertyDescriptor")
            || starts("__lookupGetter__ result")
            || starts("To")
            || starts("replace")
            || starts("array join")
            || starts("sort comparator")
            || starts("spread argument iteration")
            || starts("JSON.stringify")
            || starts("groupBy key")
            || starts("structuredClone source")
            || starts("typed-array source")
            || starts("Symbol.hasInstance")
            || starts("Reflect.get receiver")
            || w.contains(" on ")
        {
            Route::Builtin
        } else {
            return None;
        },
    )
}

/// The value a composite origin text names (`<what>: <why>`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Head {
    Param,
    This,
    Field,
    Result,
}

pub(crate) fn head(what: &str) -> Option<Head> {
    Some(
        if what == "param of escaped function"
            || what.starts_with("constructor args from")
            || what == "argument of another unit's method"
        {
            Head::Param
        } else if what == "this of escaped function"
            || what == "receiver of a method another unit calls"
        {
            Head::This
        } else if what == "result" {
            Head::Result
        } else if what.starts_with("field of escaped ")
            || what.starts_with("stored into an escaped ")
            || what.starts_with("property of an escaped function")
        {
            Head::Field
        } else {
            return None;
        },
    )
}

fn composite(origin: &str) -> Option<Unknown> {
    use Unknown::*;
    let (what, why) = origin.split_once(": ")?;
    let h = head(what)?;
    Some(match (route(why)?, h) {
        (Route::Unit, Head::Param) => UnitEdgeParameter,
        (Route::Unit, Head::This) => UnitEdgeThis,
        (Route::Unit, _) => UnitEdgeValue,
        (Route::Dynamic, _) => DynamicCode,
        (Route::Intrinsic, _) => IntrinsicPrototypeTopStore,
        (Route::Builtin, _) => OpaqueBuiltin,
        (Route::Computed, _) => ComputedKey,
        (Route::Reflective, _) => ReflectiveOperation,
        (Route::External, _) => ExternalInput,
        (Route::Buffer, _) => BufferAliasing,
        (Route::Unknown, Head::Param) => EscapedParameter,
        (Route::Unknown, Head::This) => EscapedThis,
        (Route::Unknown, Head::Field) => EscapedObjectField,
        (Route::Unknown, Head::Result) => UnknownCalleeEscape,
    })
}

/// The cause named by one seed text (`Model::add_top` / `top_val`), or
/// `None` when no member names it (a capture gap the ledger reports).
pub(crate) fn classify(origin: &str) -> Option<Unknown> {
    use Unknown::*;
    if origin.is_empty() || origin == "cycle" {
        return Some(UntracedTop);
    }
    if let Some(c) = composite(origin) {
        return Some(c);
    }
    let starts = |p: &str| origin.starts_with(p);
    Some(
        if starts("member of a builtin prototype written through")
            || starts("property of a written builtin function value")
        {
            IntrinsicPrototypeTopStore
        } else if starts("argument from another unit") || starts("argument of another unit's") {
            UnitEdgeParameter
        } else if starts("cell written by a unit over the bound") {
            WorkBound
        } else if starts("a computed require")
            || starts("module loaded by a computed specifier")
            || starts("computed-key accessor")
            || starts("accessor under a computed key")
            || origin.ends_with("[computed]")
        {
            ComputedKey
        } else if origin.contains("another unit")
            || origin.contains("other units")
            || starts("global named after the unit boundary")
            || starts("the requiring module's record")
        {
            UnitEdgeValue
        } else if starts("unresolved export * source")
            || starts("unresolved re-export source")
            || starts("unresolved scope name")
            || starts("namespace of ")
            || starts("export ")
            || starts("re-exported by ")
        {
            ModuleNamespace
        } else if starts("unregistered function id")
            || starts("accessor of unknown function value")
            || starts("unresolved class value")
        {
            FunctionWithoutIdentity
        } else if starts("result of an unknown function") {
            UnknownCalleeEscape
        } else if starts("`new` of an opaque builtin")
            || starts("result of an opaque builtin")
            || starts("result: ")
            || starts("result of unmodeled runtime entry")
            || starts("result of runtime glue")
            || starts("__lookupGetter__ result")
            || starts("structuredClone/transfer result")
            || starts("spread of an unmodeled object")
            || starts("own values of an unmodeled object")
        {
            OpaqueBuiltin
        } else if starts("global ")
            || starts("globalThis.")
            || starts("process")
            || starts("require.")
            || starts("extern value")
            || starts("native module ref")
            || starts("result of native module")
            || starts("import from native/unresolved module")
            || starts("thrown value")
        {
            ExternalInput
        } else if starts("E9 Proxy") {
            ReflectiveOperation
        } else if starts("result of unmodeled HIR construct")
            || starts(crate::gen_expr::UNMODELED_OPERAND)
            || starts("untransformed yield")
            || starts("async/generator")
            || starts("literal chunk object outside its birth")
            || starts("new.target")
            || starts("ClassEnvCurrent")
            || starts("EventEmitter bookkeeping field")
            || starts("broken native owner contract")
            || starts("conflicting native owner brands")
            || starts("unresolved `extends`")
            || starts("iteratorNextMethod")
            || starts("iteratorRestAppend")
            || starts("cjsRequireEdge")
            || starts("unknown result alternative")
        {
            UnmodeledConstruct
        } else {
            // A seed named by its escape route alone (`sort comparator
            // result`, `argument of native ...`).
            return route(origin).map(|r| match r {
                Route::Unit => UnitEdgeValue,
                Route::Unknown => UnknownCalleeEscape,
                Route::Intrinsic => IntrinsicPrototypeTopStore,
                Route::Builtin => OpaqueBuiltin,
                Route::Dynamic => DynamicCode,
                Route::Computed => ComputedKey,
                Route::Reflective => ReflectiveOperation,
                Route::External => ExternalInput,
                Route::Buffer => BufferAliasing,
            });
        },
    )
}

/// The cause a legacy verdict names by itself (its operands known): the
/// layout, key or callee obligation that failed. `None` for a proof, and
/// for the verdicts whose cause is the operand's first unknown value
/// (`receiver from unmodeled code`, the record's `causes`).
pub(crate) fn verdict_cause(v: &crate::Verdict) -> Option<Result<Unknown, String>> {
    use Unknown::*;
    let crate::Verdict::No(why) = v else {
        return None;
    };
    let w = why.as_str();
    let starts = |p: &str| w.starts_with(p);
    if starts("receiver from unmodeled code (TOP)") {
        return None;
    }
    Some(Ok(if starts("receiver is another unit's value") {
        UnitEdgeMarker
    } else if starts("dead site") {
        NoValues
    } else if let Some(e) = w.strip_prefix("escape: ") {
        match route(e) {
            Some(Route::Unit) => UnitEdgeValue,
            Some(Route::Unknown) => EscapedObjectField,
            Some(Route::Intrinsic) => IntrinsicPrototypeTopStore,
            Some(Route::Builtin) => OpaqueBuiltin,
            Some(Route::Dynamic) => DynamicCode,
            Some(Route::Computed) => ComputedKey,
            Some(Route::Reflective) => ReflectiveOperation,
            Some(Route::External) => ExternalInput,
            Some(Route::Buffer) => BufferAliasing,
            None => return Some(Err(why.clone())),
        }
    } else if starts("hard: ") || starts("chain: ") {
        HardLayout
    } else if starts("slots differ") || starts("receivers mix") {
        PolymorphicBeyondSet
    } else if starts("key not a definite") {
        KeyNotDefinite
    } else if starts("accessor key") {
        AccessorKey
    } else if starts("primitive receiver") {
        PrimitiveReceiver
    } else if starts("receiver is a builtin/function value") {
        BuiltinReceiver
    } else if starts("receiver is not a fact unit") {
        ReceiverNotALayout
    } else if starts("callee set holds ⊤") {
        TopCallee
    } else if starts("callee set holds a builtin function value") {
        OpaqueBuiltin
    } else if starts("callee set holds") {
        NonFunctionCallee
    } else if starts("not a constant static") {
        NotAConstantStatic
    } else {
        return Some(Err(why.clone()));
    }))
}

/// The site kinds of a HIR expression (the generic operations the facts
/// prove: member/element reads, calls, `new`, member/element writes).
pub(crate) fn hir_kind(e: &Expr) -> Option<&'static str> {
    Some(match e {
        Expr::PropertyGet { .. } => "property",
        Expr::IndexGet { .. } => "element",
        Expr::Call { .. } | Expr::CallSpread { .. } => "value-call",
        Expr::New { .. } | Expr::NewDynamic { .. } | Expr::NewDynamicSpread { .. } => "new",
        Expr::PropertySet { .. } => "write",
        Expr::IndexSet { .. } => "element-write",
        Expr::PropertyUpdate { .. } => "update",
        Expr::IndexUpdate { .. } => "element-update",
        _ => return None,
    })
}

fn span(e: &Expr) -> (u32, u32) {
    match e {
        Expr::PropertyGet {
            byte_offset,
            byte_end,
            ..
        }
        | Expr::IndexGet {
            byte_offset,
            byte_end,
            ..
        }
        | Expr::Call {
            byte_offset,
            byte_end,
            ..
        }
        | Expr::CallSpread {
            byte_offset,
            byte_end,
            ..
        }
        | Expr::New {
            byte_offset,
            byte_end,
            ..
        }
        | Expr::NewDynamic {
            byte_offset,
            byte_end,
            ..
        }
        | Expr::NewDynamicSpread {
            byte_offset,
            byte_end,
            ..
        }
        | Expr::PropertySet {
            byte_offset,
            byte_end,
            ..
        }
        | Expr::IndexSet {
            byte_offset,
            byte_end,
            ..
        }
        | Expr::PropertyUpdate {
            byte_offset,
            byte_end,
            ..
        }
        | Expr::IndexUpdate {
            byte_offset,
            byte_end,
            ..
        } => (*byte_offset, *byte_end),
        _ => (0, 0),
    }
}

/// The completeness census of one analysis mode (`<census>` unit): every
/// HIR site of `modules` (the HIR the analyses walked), compared with the
/// sites the analyses recorded (`recorded`, expression addresses). Each
/// site no analysis recorded gets a `site` line with its reason, and each
/// kind a `completeness` line: `hir` = `recorded` + `recordedThroughCopy`
/// + `calleeOfCall` + `unrecorded`; `recordedNotInHir` (a recorded site
/// with a source span that pairs with no HIR site) must be 0.
pub(crate) fn census(
    modules: &[(&str, &Module)],
    sites: &[(usize, &'static str, u32)],
) -> Vec<String> {
    use crate::diagnostics::quote;
    let recorded: FxHashSet<usize> = sites.iter().map(|s| s.0).collect();
    // Every HIR site: address, module, kind, span, callee position.
    let mut hir: Vec<(usize, &str, &'static str, (u32, u32), bool)> = Vec::new();
    let mut seen: FxHashSet<usize> = FxHashSet::default();
    for (path, m) in modules {
        // Member reads in callee position belong to their call's record.
        let mut callees: FxHashSet<usize> = FxHashSet::default();
        perry_hir::tdz_check::for_each_module_expr(m, &mut |e| {
            if let Expr::Call { callee, .. } | Expr::CallSpread { callee, .. } = e {
                if matches!(**callee, Expr::PropertyGet { .. } | Expr::IndexGet { .. }) {
                    callees.insert(&**callee as *const Expr as usize);
                }
            }
        });
        perry_hir::tdz_check::for_each_module_expr(m, &mut |e| {
            let Some(kind) = hir_kind(e) else {
                return;
            };
            let a = e as *const Expr as usize;
            if seen.insert(a) {
                hir.push((a, path, kind, span(e), callees.contains(&a)));
            }
        });
    }
    // A recorded expression no module holds: one the analysis made itself
    // (literal-birth stores, no source coordinate), or its own copy of a
    // HIR body (a CJS wrapper, a run-once body). A copy pairs with the
    // unrecorded HIR site of the same kind and start; the rest are gaps.
    let mut made = FxHashSet::default();
    let mut copies: BTreeMap<(&str, u32), FxHashSet<usize>> = BTreeMap::new();
    for s in sites.iter().filter(|s| !seen.contains(&s.0)) {
        if s.2 == 0 {
            made.insert(s.0);
        } else {
            copies.entry((s.1, s.2)).or_default().insert(s.0);
        }
    }
    let mut paired: BTreeMap<(&str, u32), usize> = BTreeMap::new();
    let mut out = Vec::new();
    // kind -> (hir, recorded, recorded through a copy, callee-of-call, unrecorded)
    let mut counts: BTreeMap<&'static str, [u64; 5]> = BTreeMap::new();
    for &(a, path, kind, (start, end), callee) in &hir {
        let c = counts.entry(kind).or_default();
        c[0] += 1;
        if recorded.contains(&a) {
            c[1] += 1;
            continue;
        }
        let k = (kind, start);
        let have = copies.get(&k).map_or(0, |s| s.len());
        let reason = if start != 0 && paired.get(&k).copied().unwrap_or(0) < have {
            *paired.entry(k).or_default() += 1;
            c[2] += 1;
            continue;
        } else if callee {
            c[3] += 1;
            "callee-of-call"
        } else {
            c[4] += 1;
            "not-walked"
        };
        out.push(format!(
            "{{\"record\":\"site\",\"file\":{},\"start\":{start},\"end\":{end},\"kind\":{},\"origin\":{},\"verdict\":\"not-a-site\",\"reason\":{}}}",
            quote(path),
            quote(kind),
            quote(if start == 0 { "synthesized" } else { "source" }),
            quote(reason)
        ));
    }
    let mut stray = 0;
    for (k, s) in &copies {
        let left = s.len() - paired.get(k).copied().unwrap_or(0);
        if left > 0 {
            stray += left;
            out.push(format!(
                "{{\"record\":\"stray\",\"kind\":{},\"start\":{},\"count\":{left}}}",
                quote(k.0),
                k.1
            ));
        }
    }
    for (kind, [n, rec, copy, callee, unrec]) in counts {
        out.push(format!(
            "{{\"record\":\"completeness\",\"kind\":{},\"hir\":{n},\"recorded\":{rec},\"recordedThroughCopy\":{copy},\"calleeOfCall\":{callee},\"unrecorded\":{unrec}}}",
            quote(kind)
        ));
    }
    out.push(format!(
        "{{\"record\":\"completeness\",\"kind\":\"*\",\"recordedNotInHir\":{stray},\"analysisSynthesized\":{}}}",
        made.len()
    ));
    out
}
