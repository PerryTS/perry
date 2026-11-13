//! Canonical metadata at the generation boundary; no points-to reads.
use super::*;
impl Model {
    fn packet_function(&self, f: FnIdx) -> Identity {
        let fi = &self.funcs[f as usize];
        if fi.body_packet.header.identity.0.is_empty() {
            Identity(fi.module.to_string(), fi.func_id, fi.name.clone())
        } else {
            fi.body_packet.header.identity.clone()
        }
    }

    pub(super) fn packet_rule(&self, rule: &Cons, packet: &BodyPacket) -> String {
        match *rule {
            Cons::StoreOut {
                f,
                dst,
                reach,
                via,
                key,
                reason,
                at,
                to,
            } => format!(
                "StoreOut f={:?} dst={:?} reach={:?} via={:?} key={:?} reason={:?} at={:?} to={:?}",
                self.packet_function(f),
                dst,
                reach,
                via.map(|a| self.atoms[a as usize].clone()),
                self.packet_key(key),
                self.reasons.get(reason as usize),
                self.packet_token(at, packet),
                self.packet_token(to, packet)
            ),
            Cons::ProtoKey { key, dst } => format!(
                "ProtoKey key={:?} dst={:?}",
                self.atoms[key as usize].clone(),
                dst
            ),
            Cons::Load { key, dst } => format!("Load key={:?} dst={:?}", self.packet_key(key), dst),
            Cons::OwnValues { dst } => format!("OwnValues dst={:?}", dst),
            Cons::IndexHole => format!("IndexHole"),
            Cons::IndexKept => format!("IndexKept"),
            Cons::Identity { route } => format!("Identity route={}", route.name()),
            Cons::LenIndex { lo, hi } => format!("LenIndex {lo} {hi}"),
            Cons::LenPermuted { t } => format!("LenPermuted {t}"),
            Cons::CjsPublish { dst } => format!("CjsPublish dst={:?}", dst),
            Cons::Store { key, src, site } => format!(
                "Store key={:?} src={:?} site={:?}",
                self.packet_key(key),
                src,
                self.stores
                    .get(site as usize)
                    .map(|s| (&s.desc, s.birth, s.rooted))
            ),
            Cons::Call { call } => format!("Call call={:?}", call),
            Cons::CallThis { call, this } => format!(
                "CallThis call={:?} this={:?}",
                call,
                self.packet_token(this, packet)
            ),
            Cons::New { call } => format!("New call={:?}", call),
            Cons::NewProgram { call } => format!("NewProgram call={:?}", call),
            Cons::FromArrayLike { dst } => format!("FromArrayLike dst={:?}", dst),
            Cons::ReflectNew { call, new_target } => {
                format!("ReflectNew call={:?} new_target={:?}", call, new_target)
            }
            Cons::ReflectNewTarget { call, target } => format!(
                "ReflectNewTarget call={:?} target={:?}",
                call,
                self.packet_token(target, packet)
            ),
            Cons::NewNamed { name, call } => format!(
                "NewNamed name={:?} call={:?}",
                self.atoms[name as usize].clone(),
                call
            ),
            Cons::MCall { key, call } => {
                format!("MCall key={:?} call={:?}", self.packet_key(key), call)
            }
            Cons::Chain { origin, key, act } => format!(
                "Chain origin={:?} key={:?} act={:?}",
                self.packet_token(origin, packet),
                self.packet_key(key),
                act
            ),
            Cons::Raise { region } => format!("Raise region={region}"),
            Cons::TopSink { reason } => {
                format!("TopSink reason={:?}", self.reasons.get(reason as usize))
            }
            Cons::RuntimeStore { reason, retained } => format!(
                "RuntimeStore reason={:?} retained={:?}",
                self.reasons.get(reason as usize),
                self.packet_token(retained, packet)
            ),
            Cons::HandOver { reason, ctor, arg } => format!(
                "HandOver reason={:?} ctor={:?} arg={:?}",
                self.reasons.get(reason as usize),
                self.packet_token(ctor, packet),
                arg
            ),
            Cons::OwnOnly { f, dst } => {
                format!("OwnOnly f={:?} dst={:?}", self.packet_function(f), dst)
            }
            Cons::CallerValue { dst, reason } => format!(
                "CallerValue dst={:?} reason={:?}",
                dst,
                self.reasons.get(reason as usize)
            ),
            Cons::ContractThis {
                tmpl,
                reason,
                statics,
                this,
            } => format!(
                "ContractThis tmpl={:?} reason={:?} statics={:?} this={:?}",
                self.tmpls[tmpl as usize].display.clone(),
                self.reasons.get(reason as usize),
                statics,
                this
            ),
            Cons::OverrideThis { tmpl, key, reason } => format!(
                "OverrideThis tmpl={:?} key={:?} reason={:?}",
                self.tmpls[tmpl as usize].display.clone(),
                self.atoms[key as usize].clone(),
                self.reasons.get(reason as usize)
            ),
            Cons::TopIfAny { dst, from, reason } => format!(
                "TopIfAny dst={:?} from={:?} reason={:?}",
                dst,
                from,
                reason.and_then(|r| self.reasons.get(r as usize))
            ),
            Cons::RequireSpec { call } => format!("RequireSpec call={:?}", call),
            Cons::TopIfTop { dst, reason } => format!(
                "TopIfTop dst={:?} reason={:?}",
                dst,
                self.reasons.get(reason as usize)
            ),
            Cons::Guard { test, from, dst } => {
                format!("Guard test={:?} from={:?} dst={:?}", test, from, dst)
            }
            Cons::Defined { dst } => format!("Defined dst={:?}", dst),
            Cons::NotNullish { dst } => format!("NotNullish dst={:?}", dst),
            Cons::FieldTruthy { key, test, dst } => {
                format!("FieldTruthy key={:?} test={} dst={:?}", key, test, dst)
            }
            Cons::Fulfil { dst } => format!("Fulfil dst={:?}", dst),
            Cons::Falsy { dst } => format!("Falsy dst={:?}", dst),
            Cons::NotUndefined { dst } => format!("NotUndefined dst={:?}", dst),
            Cons::ProtoObjects { dst } => format!("ProtoObjects dst={:?}", dst),
            Cons::NotBuffer { shared, dst } => {
                format!("NotBuffer shared={shared} dst={:?}", dst)
            }
            Cons::Hard { kind, reason } => format!(
                "Hard kind={:?} reason={:?}",
                kind,
                self.reasons.get(reason as usize)
            ),
            Cons::Miss { key } => format!("Miss key={:?}", self.packet_key(key)),
            Cons::DeleteKey { key, reason } => format!(
                "DeleteKey key={:?} reason={:?}",
                self.atoms[key as usize].clone(),
                self.reasons.get(reason as usize)
            ),
            Cons::DefineKey { key, data, reason } => format!(
                "DefineKey key={:?} data={:?} reason={:?}",
                self.atoms[key as usize].clone(),
                data,
                self.reasons.get(reason as usize)
            ),
            Cons::DefineAll {
                o,
                descs,
                value,
                site,
                reason,
            } => format!(
                "DefineAll o={:?} descs={:?} value={:?} site={:?} reason={:?}",
                o,
                descs,
                value,
                self.stores
                    .get(site as usize)
                    .map(|s| (&s.desc, s.birth, s.rooted)),
                self.reasons.get(reason as usize)
            ),
            Cons::DefineDesc {
                o,
                key,
                site,
                reason,
            } => format!(
                "DefineDesc o={:?} key={:?} site={:?} reason={:?}",
                o,
                self.atoms[key as usize].clone(),
                self.stores
                    .get(site as usize)
                    .map(|s| (&s.desc, s.birth, s.rooted)),
                self.reasons.get(reason as usize)
            ),
            Cons::AddsKeys => format!("AddsKeys"),
            Cons::Implicit { reason, proto } => format!(
                "Implicit reason={:?} proto={:?}",
                self.reasons.get(reason as usize),
                proto
            ),
            Cons::HasInstance { reason, arg } => format!(
                "HasInstance reason={:?} arg={arg}",
                self.reasons.get(reason as usize)
            ),
            Cons::CreateProto { reason } => {
                format!("CreateProto reason={:?}", self.reasons.get(reason as usize))
            }
            Cons::ChainUnion {
                key,
                dst,
                of,
                builtin,
            } => format!(
                "ChainUnion key={:?} dst={:?} of={:?} builtin={:?}",
                self.packet_key(key),
                dst,
                self.packet_token(of, packet),
                builtin
            ),
            Cons::ProtoGiven { protos } => format!("ProtoGiven protos={:?}", protos),
            Cons::ParentGiven { of } => {
                format!("ParentGiven of={:?}", self.packet_token(of, packet))
            }
            Cons::ProtoClosure { dst } => format!("ProtoClosure dst={:?}", dst),
            Cons::ProtoMethods { origin, key, call } => format!(
                "ProtoMethods origin={:?} key={:?} call={:?}",
                self.packet_token(origin, packet),
                self.packet_key(key),
                call
            ),
            Cons::AnyAccessors { origin, act } => format!(
                "AnyAccessors origin={:?} act={:?}",
                self.packet_token(origin, packet),
                act
            ),
            Cons::UnnamedSet { target } => {
                format!("UnnamedSet target={:?}", self.packet_token(target, packet))
            }
            Cons::StoreTopProto { origin, src } => format!(
                "StoreTopProto origin={:?} src={:?}",
                self.packet_token(origin, packet),
                src
            ),
            Cons::Super { start, key, act } => format!(
                "Super start={:?} key={:?} act={:?}",
                self.packet_token(start, packet),
                self.packet_key(key),
                act
            ),
            Cons::SuperFrom { recv, key, act } => format!(
                "SuperFrom recv={:?} key={:?} act={:?}",
                recv,
                self.packet_key(key),
                act
            ),
            Cons::SuperCtor { class, call } => format!(
                "SuperCtor class={:?} call={:?}",
                self.tmpls[class as usize].display.clone(),
                call
            ),
            Cons::SuperCtorParent {
                class,
                call,
                origin,
            } => format!(
                "SuperCtorParent class={:?} call={:?} origin={:?}",
                self.tmpls[class as usize].display.clone(),
                call,
                self.packet_token(origin, packet)
            ),
            Cons::ThisFrom { recv } => format!("ThisFrom recv={:?}", recv),
            Cons::BirthField { key, src } => {
                format!("BirthField key={:?} src={:?}", self.packet_key(key), src)
            }
            Cons::Accessor {
                holder,
                key,
                setter,
            } => format!(
                "Accessor holder={:?} key={:?} setter={:?}",
                holder,
                self.atoms[key as usize].clone(),
                setter
            ),
            Cons::AccessorInstall { key, f, setter, .. } => format!(
                "AccessorInstall key={:?} f={:?} setter={:?}",
                self.atoms[key as usize].clone(),
                self.packet_token(f, packet),
                setter
            ),
            Cons::AnnexBDefine { call, setter } => {
                format!("AnnexBDefine call={:?} setter={:?}", call, setter)
            }
            Cons::AnnexBLookup { call, setter } => {
                format!("AnnexBLookup call={:?} setter={:?}", call, setter)
            }
            Cons::ProtoFrom { src } => format!("ProtoFrom src={:?}", src),
            Cons::ProtoOf { dst } => format!("ProtoOf dst={:?}", dst),
            Cons::ProtoValues { dst } => format!("ProtoValues dst={:?}", dst),
            Cons::PlainKey { origin, act } => format!(
                "PlainKey origin={:?} act={:?}",
                self.packet_token(origin, packet),
                act
            ),
            Cons::IterNext { dst } => format!("IterNext dst={:?}", dst),
            Cons::NativeParent {
                tm,
                expected,
                reason,
            } => format!(
                "NativeParent template={:?} expected={:?} reason={:?}",
                self.tmpls[tm as usize].display,
                self.packet_token(expected, packet),
                self.reasons.get(reason as usize)
            ),
            Cons::NewTargetHard { reason } => format!(
                "NewTargetHard reason={:?}",
                self.reasons.get(reason as usize)
            ),
            Cons::IdxLoad {
                idx,
                dst,
                shape,
                enumerable,
            } => {
                format!(
                    "IdxLoad idx={:?} dst={:?} shape={:?} enumerable={enumerable}",
                    idx,
                    dst,
                    self.packet_shape(shape)
                )
            }
            Cons::EnumChain { key, dst, of, .. } => format!(
                "EnumChain key={:?} dst={:?} of={:?}",
                key,
                dst,
                self.packet_token(of, packet)
            ),
            Cons::ElemUndef { obj, key, dst } => format!(
                "ElemUndef obj={:?} key={:?} dst={:?}",
                self.packet_token(obj, packet),
                key,
                dst
            ),
            Cons::IdxKey { obj, dst, shape } => format!(
                "IdxKey obj={:?} dst={:?} shape={:?}",
                self.packet_token(obj, packet),
                dst,
                self.packet_shape(shape)
            ),
            Cons::IdxOwnLoad {
                idx,
                dst,
                shape,
                enumerable,
            } => {
                format!(
                    "IdxOwnLoad idx={:?} dst={:?} shape={:?} enumerable={enumerable}",
                    idx,
                    dst,
                    self.packet_shape(shape)
                )
            }
            Cons::IdxOwnKey { obj, dst, shape } => format!(
                "IdxOwnKey obj={:?} dst={:?} shape={:?}",
                self.packet_token(obj, packet),
                dst,
                self.packet_shape(shape)
            ),
            Cons::OwnTest { dst, at } => format!("OwnTest dst={:?} at={:?}", dst, at),
            Cons::OwnGate {
                obj,
                idx,
                dst,
                shape,
                enumerable,
            } => format!(
                "OwnGate obj={:?} idx={:?} dst={:?} shape={:?} enumerable={enumerable}",
                obj,
                idx,
                dst,
                self.packet_shape(shape)
            ),
            Cons::IdxMCall { idx, call, shape } => {
                format!(
                    "IdxMCall idx={:?} call={:?} shape={:?}",
                    idx,
                    call,
                    self.packet_shape(shape)
                )
            }
            Cons::IdxKeyCall {
                obj,
                prim,
                call,
                shape,
            } => format!(
                "IdxKeyCall obj={:?} prim={:?} call={:?} shape={:?}",
                self.packet_token(obj, packet),
                prim,
                call,
                self.packet_shape(shape)
            ),
            Cons::ForInBody { obj, call } => format!("ForInBody obj={:?} call={:?}", obj, call),
            Cons::ForInKeys { ft, call } => format!(
                "ForInKeys ft={:?} call={:?}",
                self.packet_token(ft, packet),
                call
            ),
            Cons::DefineKeyed {
                o,
                reason,
                accessor,
            } => format!(
                "DefineKeyed o={:?} reason={:?} accessor={:?}",
                o,
                self.reasons.get(reason as usize),
                accessor
            ),
            Cons::EnumKeyStore { o, src, site, from } => format!(
                "EnumKeyStore o={:?} src={:?} site={site} from={from:?}",
                o, src
            ),
            Cons::EnumKeyAny { o, src, site } => {
                format!("EnumKeyAny o={:?} src={:?} site={site}", o, src)
            }
            Cons::StoreKeyed { o, src, site, .. } => format!(
                "StoreKeyed o={:?} src={:?} site={:?}",
                o,
                src,
                self.stores
                    .get(site as usize)
                    .map(|s| (&s.desc, s.birth, s.rooted))
            ),
            Cons::StoreIndexed {
                o,
                src,
                site,
                shape,
            } => format!(
                "StoreIndexed o={:?} src={:?} site={:?} shape={:?}",
                o,
                src,
                self.stores
                    .get(site as usize)
                    .map(|s| (&s.desc, s.birth, s.rooted)),
                self.packet_shape(shape)
            ),
            Cons::AccessorKeyed { holder, f, setter } => format!(
                "AccessorKeyed holder={:?} f={:?} setter={:?}",
                holder, f, setter
            ),
            Cons::Arith { dst, .. } => format!("Arith dst={:?}", dst),
            Cons::Range {
                op,
                other,
                dst,
                right,
            } => format!(
                "Range op={} other={:?} dst={:?} right={}",
                op.name(),
                other,
                dst,
                right
            ),
            Cons::Plus { str, num, .. } => format!("Plus str={:?} num={:?}", str, num),
            Cons::ArrayLenArg { dst } => format!("ArrayLenArg dst={:?}", dst),
            Cons::CaseMap { dst, upper } => format!("CaseMap dst={:?} upper={upper}", dst),
            Cons::BoundCall { reason, bound } => format!(
                "BoundCall reason={:?} bound={:?}",
                self.reasons.get(reason as usize),
                self.packet_token(bound, packet)
            ),
            Cons::ShareBuffer { ta } => {
                format!("ShareBuffer ta={:?}", self.packet_token(ta, packet))
            }
            Cons::StrReplace {
                dst,
                recv,
                search,
                repl,
            } => format!("StrReplace dst={dst:?} recv={recv:?} search={search:?} repl={repl:?}"),
            Cons::RegExpPattern { tok } => {
                format!("RegExpPattern tok={:?}", self.packet_token(tok, packet))
            }
            Cons::Captures { rec, pat } => {
                format!("Captures rec={:?} pat={pat}", self.packet_token(rec, packet))
            }
            Cons::ViewedBuffer { view, fresh, share } => format!(
                "ViewedBuffer view={:?} fresh={fresh:?} share={share}",
                self.packet_token(view, packet)
            ),
            Cons::ThisTo { origin } => {
                format!("ThisTo origin={:?}", self.packet_token(origin, packet))
            }
            Cons::CallArgs { call } => format!("CallArgs call={:?}", call),
            Cons::RetInto { call, run } => format!("RetInto call={:?} run={:?}", call, run),
            Cons::ArgInto { call, run, i } => {
                format!("ArgInto call={:?} run={:?} i={:?}", call, run, i)
            }
            Cons::Spread { into, site } => format!(
                "Spread into={:?} site={:?}",
                into,
                self.stores
                    .get(site as usize)
                    .map(|s| (&s.desc, s.birth, s.rooted))
            ),
            Cons::SpreadFrom { src, site, .. } => format!(
                "SpreadFrom src={:?} site={:?}",
                self.packet_token(src, packet),
                self.stores
                    .get(site as usize)
                    .map(|s| (&s.desc, s.birth, s.rooted))
            ),
            Cons::ThisInst { call, ft } => format!(
                "ThisInst call={:?} ft={:?}",
                call,
                self.packet_token(ft, packet)
            ),
            Cons::NativeThis { call, f } => format!(
                "NativeThis call={:?} f={:?}",
                call,
                self.packet_token(f, packet)
            ),
            Cons::BuiltinCallee => format!("BuiltinCallee"),
            Cons::HolderStep { key, pass, stop } => format!(
                "HolderStep key={:?} pass={:?} stop={:?}",
                self.packet_key(key),
                pass,
                stop
            ),
            Cons::VisitHolders { origin, key, act } => format!(
                "VisitHolders origin={:?} key={:?} act={:?}",
                self.packet_token(origin, packet),
                self.packet_key(key),
                act
            ),
            Cons::VisitStop { origin, key, act } => format!(
                "VisitStop origin={:?} key={:?} act={:?}",
                self.packet_token(origin, packet),
                self.packet_key(key),
                act
            ),
        }
    }
}
