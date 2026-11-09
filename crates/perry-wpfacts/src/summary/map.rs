//! Remapping a constraint or call site: the terms of a summary (slots, call templates) into nodes and back.

use super::*;

pub(super) fn is_slot(x: NodeId) -> bool {
    x != NO_NODE && x & SLOT != 0
}

pub(super) fn act_map(
    a: ChainAct,
    n: &mut impl FnMut(NodeId) -> NodeId,
    k: &mut impl FnMut(CallId) -> CallId,
) -> ChainAct {
    match a {
        ChainAct::Lookup { dst, setter } => ChainAct::Lookup {
            dst: n(dst),
            setter,
        },
        ChainAct::Load(x) => ChainAct::Load(n(x)),
        ChainAct::Store(x) => ChainAct::Store(n(x)),
        ChainAct::Call(c) => ChainAct::Call(k(c)),
    }
}

/// Rename every node and call field of a constraint (every variant).
pub fn map_cons(
    c: &Cons,
    mut n: impl FnMut(NodeId) -> NodeId,
    mut k: impl FnMut(CallId) -> CallId,
) -> Cons {
    use Cons::*;
    match *c {
        Load { key, dst } => Load { key, dst: n(dst) },
        ProtoKey { key, dst } => ProtoKey { key, dst: n(dst) },
        OwnValues { dst } => OwnValues { dst: n(dst) },
        IndexHole => IndexHole,
        LenIndex { lo, hi } => LenIndex { lo, hi },
        LenPermuted { t } => LenPermuted { t },
        CjsPublish { dst } => CjsPublish { dst: n(dst) },
        Store { key, src, site } => Store {
            key,
            src: n(src),
            site,
        },
        Call { call } => Call { call: k(call) },
        CallThis { call, this } => CallThis {
            call: k(call),
            this,
        },
        New { call } => New { call: k(call) },
        NewProgram { call } => NewProgram { call: k(call) },
        FromArrayLike { dst } => FromArrayLike { dst: n(dst) },
        ReflectNew { call, new_target } => ReflectNew {
            call: k(call),
            new_target: n(new_target),
        },
        ReflectNewTarget { call, target } => ReflectNewTarget {
            call: k(call),
            target,
        },
        NewNamed { name, call } => NewNamed {
            name,
            call: k(call),
        },
        MCall { key, call } => MCall { key, call: k(call) },
        Chain { origin, key, act } => Chain {
            origin,
            key,
            act: act_map(act, &mut n, &mut k),
        },
        TopSink { reason } => TopSink { reason },
        Raise { region } => Raise { region: n(region) },
        RuntimeStore { reason, retained } => RuntimeStore { reason, retained },
        HandOver { reason, ctor, arg } => HandOver { reason, ctor, arg },
        OwnOnly { f, dst } => OwnOnly { f, dst: n(dst) },
        CallerValue { dst, reason } => CallerValue {
            dst: n(dst),
            reason,
        },
        StoreOut {
            f,
            dst,
            reach,
            via,
            key,
            reason,
            at,
            to,
        } => StoreOut {
            f,
            dst,
            reach,
            via,
            key,
            reason,
            at,
            to,
        },
        ContractThis {
            tmpl,
            reason,
            statics,
            this,
        } => ContractThis {
            tmpl,
            reason,
            statics,
            this,
        },
        OverrideThis { tmpl, key, reason } => OverrideThis { tmpl, key, reason },
        Guard { test, from, dst } => Guard {
            test,
            from: n(from),
            dst: n(dst),
        },
        Defined { dst } => Defined { dst: n(dst) },
        NotNullish { dst } => NotNullish { dst: n(dst) },
        Falsy { dst } => Falsy { dst: n(dst) },
        NotUndefined { dst } => NotUndefined { dst: n(dst) },
        TopIfAny { dst, from, reason } => TopIfAny {
            dst: n(dst),
            from: n(from),
            reason,
        },
        Hard { kind, reason } => Hard { kind, reason },
        Miss { key } => Miss { key },
        DeleteKey { key, reason } => DeleteKey { key, reason },
        AddsKeys => AddsKeys,
        DefineKey { key, data, reason } => DefineKey { key, data, reason },
        DefineAll {
            o,
            descs,
            value,
            site,
            reason,
        } => DefineAll {
            o: n(o),
            descs: n(descs),
            value: n(value),
            site,
            reason,
        },
        DefineDesc {
            o,
            key,
            site,
            reason,
        } => DefineDesc {
            o: n(o),
            key,
            site,
            reason,
        },
        Implicit { reason, proto } => Implicit { reason, proto },
        HasInstance { reason, arg } => HasInstance {
            reason,
            arg: n(arg),
        },
        CreateProto { reason } => CreateProto { reason },
        ChainUnion {
            key,
            dst,
            of,
            builtin,
        } => ChainUnion {
            key,
            dst: n(dst),
            of,
            builtin,
        },
        ProtoGiven { protos } => ProtoGiven { protos: n(protos) },
        ParentGiven { of } => ParentGiven { of },
        ProtoClosure { dst } => ProtoClosure { dst: n(dst) },
        ProtoMethods { origin, key, call } => ProtoMethods {
            origin,
            key,
            call: k(call),
        },
        AnyAccessors { origin, act } => AnyAccessors {
            origin,
            act: act_map(act, &mut n, &mut k),
        },
        UnnamedSet { target } => UnnamedSet { target },
        PlainKey { origin, act } => PlainKey {
            origin,
            act: act_map(act, &mut n, &mut k),
        },
        StoreTopProto { origin, src } => StoreTopProto {
            origin,
            src: n(src),
        },
        Super { start, key, act } => Super {
            start,
            key,
            act: act_map(act, &mut n, &mut k),
        },
        SuperFrom { recv, key, act } => SuperFrom {
            recv: n(recv),
            key,
            act: act_map(act, &mut n, &mut k),
        },
        SuperCtor { class, call } => SuperCtor {
            class,
            call: k(call),
        },
        SuperCtorParent {
            class,
            call,
            origin,
        } => SuperCtorParent {
            class,
            call: k(call),
            origin,
        },
        ThisFrom { recv } => ThisFrom { recv: n(recv) },
        BirthField { key, src } => BirthField { key, src: n(src) },
        Accessor {
            holder,
            key,
            setter,
        } => Accessor {
            holder: n(holder),
            key,
            setter,
        },
        AccessorInstall {
            key,
            f,
            setter,
            from,
        } => AccessorInstall {
            key,
            f,
            setter,
            from: if from == NO_NODE { NO_NODE } else { n(from) },
        },
        AnnexBDefine { call, setter } => AnnexBDefine {
            call: k(call),
            setter,
        },
        AnnexBLookup { call, setter } => AnnexBLookup {
            call: k(call),
            setter,
        },
        ProtoFrom { src } => ProtoFrom { src: n(src) },
        ProtoOf { dst } => ProtoOf { dst: n(dst) },
        ProtoValues { dst } => ProtoValues { dst: n(dst) },
        IterNext { dst } => IterNext { dst: n(dst) },
        NewTargetHard { reason } => NewTargetHard { reason },
        NativeParent {
            tm,
            expected,
            reason,
        } => NativeParent {
            tm,
            expected,
            reason,
        },
        IdxLoad { idx, dst, shape } => IdxLoad {
            idx: n(idx),
            dst: n(dst),
            shape,
        },
        IdxKey { obj, dst, shape } => IdxKey {
            obj,
            dst: n(dst),
            shape,
        },
        IdxOwnLoad { idx, dst, shape } => IdxOwnLoad {
            idx: n(idx),
            dst: n(dst),
            shape,
        },
        IdxOwnKey { obj, dst, shape } => IdxOwnKey {
            obj,
            dst: n(dst),
            shape,
        },
        OwnTest { dst, at } => OwnTest { dst: n(dst), at },
        OwnGate {
            obj,
            idx,
            dst,
            shape,
        } => OwnGate {
            obj: n(obj),
            idx: n(idx),
            dst: n(dst),
            shape,
        },
        IdxMCall { idx, call, shape } => IdxMCall {
            idx: n(idx),
            call: k(call),
            shape,
        },
        IdxKeyCall {
            obj,
            prim,
            call,
            shape,
        } => IdxKeyCall {
            obj,
            prim,
            call: k(call),
            shape,
        },
        ForInBody { obj, call } => ForInBody {
            obj: n(obj),
            call: k(call),
        },
        ForInKeys { ft, call } => ForInKeys { ft, call: k(call) },
        DefineKeyed {
            o,
            reason,
            accessor,
        } => DefineKeyed {
            o: n(o),
            reason,
            accessor,
        },
        StoreKeyed { o, src, site } => StoreKeyed {
            o: n(o),
            src: n(src),
            site,
        },
        StoreIndexed {
            o,
            src,
            site,
            shape,
        } => StoreIndexed {
            o: n(o),
            src: n(src),
            site,
            shape,
        },
        AccessorKeyed { holder, f, setter } => AccessorKeyed {
            holder: n(holder),
            f: n(f),
            setter,
        },
        Arith { dst } => Arith { dst: n(dst) },
        Range {
            op,
            other,
            dst,
            right,
        } => Range {
            op,
            other: if other == NO_NODE { NO_NODE } else { n(other) },
            dst: n(dst),
            right,
        },
        Plus { str, num } => Plus {
            str: n(str),
            num: n(num),
        },
        ArrayLenArg { dst } => ArrayLenArg { dst: n(dst) },
        ShareBuffer { ta } => ShareBuffer { ta },
        ThisTo { origin } => ThisTo { origin },
        CallArgs { call } => CallArgs { call: k(call) },
        RetInto { call, run } => RetInto { call: k(call), run },
        ArgInto { call, run, i } => ArgInto {
            call: k(call),
            run,
            i,
        },
        Spread { into, site } => Spread {
            into: n(into),
            site,
        },
        SpreadFrom { src, site, from } => SpreadFrom {
            src,
            site,
            from: if from == NO_NODE { NO_NODE } else { n(from) },
        },
        ThisInst { call, ft } => ThisInst { call: k(call), ft },
        NativeThis { call, f } => NativeThis { call: k(call), f },
        BuiltinCallee => BuiltinCallee,
        HolderStep { key, pass, stop } => HolderStep {
            key,
            pass: n(pass),
            stop: n(stop),
        },
        VisitHolders { origin, key, act } => VisitHolders {
            origin,
            key,
            act: act_map(act, &mut n, &mut k),
        },
        VisitStop { origin, key, act } => VisitStop {
            origin,
            key,
            act: act_map(act, &mut n, &mut k),
        },
    }
}

pub fn map_call(c: &CallSite, mut n: impl FnMut(NodeId) -> NodeId) -> CallSite {
    CallSite {
        this: match c.this {
            ThisArg::Node(x) => ThisArg::Node(n(x)),
            ThisArg::Tok(t) => ThisArg::Tok(t),
            ThisArg::Undef => ThisArg::Undef,
        },
        args: c.args.iter().map(|&x| n(x)).collect(),
        spread: c.spread.iter().map(|&x| n(x)).collect(),
        ret: n(c.ret),
        site: c.site,
        region: n(c.region),
    }
}

pub(super) fn cons_fields(c: &Cons) -> (Vec<NodeId>, Vec<CallId>) {
    let mut ns = Vec::new();
    let mut ks = Vec::new();
    map_cons(
        c,
        |x| {
            ns.push(x);
            x
        },
        |k| {
            ks.push(k);
            k
        },
    );
    (ns, ks)
}

pub(super) fn call_nodes(c: &CallSite) -> Vec<NodeId> {
    let mut v = Vec::new();
    map_call(c, |x| {
        v.push(x);
        x
    });
    v
}

/// The constraint kinds generation attaches to program nodes: the summary
/// language. The solver's own relays are rebuilt by applying these.
pub(super) fn gen_kind(c: &Cons) -> bool {
    use Cons::*;
    matches!(
        c,
        Load { .. }
            | ProtoKey { .. }
            | OwnValues { .. }
            | CjsPublish { .. }
            | Spread { .. }
            | Store { .. }
            | Call { .. }
            | New { .. }
            | NewProgram { .. }
            | FromArrayLike { .. }
            | ReflectNew { .. }
            | NewNamed { .. }
            | SuperCtor { .. }
            | MCall { .. }
            | TopSink { .. }
            | Raise { .. }
            | RuntimeStore { .. }
            | HandOver { .. }
            | ContractThis { .. }
            | OverrideThis { .. }
            | TopIfAny { .. }
            | Guard { .. }
            | NotNullish { .. }
            | Falsy { .. }
            | NotUndefined { .. }
            | Hard { .. }
            | Miss { .. }
            | DeleteKey { .. }
            | DefineKey { .. }
            | DefineAll { .. }
            | AddsKeys
            | Implicit { .. }
            | HasInstance { .. }
            | IdxLoad { .. }
            | IdxKey { .. }
            | IdxOwnLoad { .. }
            | IdxOwnKey { .. }
            | OwnGate { .. }
            | OwnTest { .. }
            | IdxMCall { .. }
            | IdxKeyCall { .. }
            | ForInBody { .. }
            | DefineKeyed { .. }
            | StoreKeyed { .. }
            | StoreIndexed { .. }
            | AccessorKeyed { .. }
            | Arith { .. }
            | Range { .. }
            | LenIndex { .. }
            | Plus { .. }
            | ArrayLenArg { .. }
            | IterNext { .. }
            | ProtoOf { .. }
            | ThisFrom { .. }
            | BirthField { .. }
            | CreateProto { .. }
            | NewTargetHard { .. }
            | NativeParent { .. }
            | ShareBuffer { .. }
            | AnnexBDefine { .. }
            | AnnexBLookup { .. }
            | Accessor { .. }
            | ProtoFrom { .. }
    )
}

/// Read rules: their only output is `dst`; per-token effects (getters,
/// protocols) are the same for every call. (`Plus` has two outputs and is
/// none here: it is never dropped as a dead read.)
pub(super) fn read_dst(c: &Cons) -> Option<NodeId> {
    use Cons::*;
    match *c {
        Load { dst, .. }
        | ProtoKey { dst, .. }
        | OwnValues { dst }
        | CjsPublish { dst }
        | IdxLoad { dst, .. }
        | IdxKey { dst, .. }
        | IdxOwnLoad { dst, .. }
        | IdxOwnKey { dst, .. }
        | OwnTest { dst, .. }
        | Arith { dst }
        | Range { dst, .. }
        | ArrayLenArg { dst }
        | IterNext { dst }
        | FromArrayLike { dst }
        | ProtoOf { dst }
        | ProtoValues { dst } => Some(dst),
        _ => None,
    }
}
