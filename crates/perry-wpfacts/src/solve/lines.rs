//! The text forms of constraints and tokens the dumps and reports print.

use super::*;

impl Model {
    pub fn cons_line(&self, n: NodeId, c: &Cons) -> String {
        match c {
            Cons::Identity { route } => format!("identity {n} {}", route.name()),
            Cons::HasProp { key, idx } => format!("hasprop {n} {key:?} {idx}"),
            Cons::Load { key, dst } => format!("load {n} {} {dst}", self.key_name(*key)),
            Cons::ProtoKey { key, dst } => {
                format!("protokey {n} {} {dst}", self.key_name(Key::Named(*key)))
            }
            Cons::OwnValues { dst } => format!("ownvalues {n} {dst}"),
            Cons::OwnDescValues { dst } => format!("owndescvalues {n} {dst}"),
            Cons::IndexHole => format!("indexhole {n}"),
            Cons::IndexHoleIn { key } => format!("indexholein {n} {key}"),
            Cons::IndexKept => format!("indexkept {n}"),
            Cons::LenIndex { lo, hi } => format!("lenindex {n} {lo} {hi}"),
            Cons::LenPermuted { t } => format!("lenpermuted {n} {t}"),
            Cons::CjsPublish { dst } => format!("cjspublish {n} {dst}"),
            Cons::Store { key, src, site } => {
                format!("store {n} {} {src} {site}", self.key_name(*key))
            }
            Cons::Call { call } => format!("call {n} {call}"),
            Cons::CallThis { call, this } => format!("callthis {n} {call} {this}"),
            Cons::New { call } => format!("new {n} {call}"),
            Cons::NewProgram { call } => format!("newprogram {n} {call}"),
            Cons::FromArrayLike { dst } => format!("fromarraylike {n} {dst}"),
            Cons::ReflectNew { call, new_target } => format!("reflectnew {n} {call} {new_target}"),
            Cons::ReflectNewTarget { call, target } => {
                format!("reflectnewtarget {n} {call} {target}")
            }
            Cons::NewNamed { name, call } => format!(
                "newnamed {n} {} {call}",
                esc_key(&self.atoms[*name as usize])
            ),
            Cons::MCall { key, call } => format!("mcall {n} {} {call}", self.key_name(*key)),
            Cons::Chain { origin, key, act } => {
                format!("chain {n} {origin} {} {act:?}", self.key_name(*key))
            }
            Cons::TopSink { reason } => format!("top {n} {reason}"),
            Cons::Raise { region } => format!("raise {n} {region}"),
            Cons::RuntimeStore { reason, retained } => {
                format!("runtimestore {n} {retained} {reason}")
            }
            Cons::HandOver { reason, ctor, arg } => format!("handover {n} {reason} {ctor} {arg}"),
            Cons::OwnOnly { f, dst } => format!("ownonly {n} {f} {dst}"),
            Cons::CallerValue { dst, reason } => format!("callervalue {n} {dst} {reason}"),
            Cons::StoreOut {
                f,
                dst,
                reach,
                via,
                key,
                reason,
                ..
            } => format!(
                "storeout {n} {f} {dst} {reach} {} {} {reason}",
                via.map_or("-".to_string(), |v| esc_key(&self.atoms[v as usize])),
                self.key_name(*key)
            ),
            Cons::ContractThis {
                tmpl,
                reason,
                statics,
                this,
            } => format!(
                "contractthis {n} {tmpl} {reason} {} {}",
                *statics as u8, *this as i64
            ),
            Cons::OverrideThis { tmpl, key, reason } => format!(
                "overridethis {n} {tmpl} {} {reason}",
                self.key_name(Key::Named(*key))
            ),
            Cons::Guard { test, from, dst } => format!("guard {n} {test} {from} {dst}"),
            Cons::Defined { dst } => format!("defined {n} {dst}"),
            Cons::NotNullish { dst } => format!("notnullish {n} {dst}"),
            Cons::FieldTruthy { key, test, dst } => {
                format!("fieldtruthy {n} {} {test} {dst}", self.key_name(*key))
            }
            Cons::Fulfil { dst } => format!("fulfil {n} {dst}"),
            Cons::Falsy { dst } => format!("falsy {n} {dst}"),
            Cons::NotUndefined { dst } => format!("notundefined {n} {dst}"),
            Cons::NotNull { dst } => format!("notnull {n} {dst}"),
            Cons::TypeIs { mask, dst } => format!("typeis {n} {mask:#x} {dst}"),
            Cons::NotBuffer { shared, dst } => format!("notbuffer {n} {dst} shared={shared}"),
            Cons::ProtoObjects { dst } => format!("protoobjects {n} {dst}"),
            Cons::Objects { dst } => format!("objects {n} {dst}"),
            Cons::TopIfAny { dst, from, reason } => format!(
                "topifany {n} {dst} {from} {}",
                reason.map_or(-1, |r| r as i64)
            ),
            Cons::RequireSpec { call } => format!("requirespec {n} {call}"),
            Cons::TopIfTop { dst, reason } => format!("topiftop {n} {dst} {reason}"),
            Cons::Hard { kind, reason } => format!("hard {n} {} {reason}", kind.code()),
            Cons::DeleteKey { key, reason } => {
                format!(
                    "deletekey {n} {} {reason}",
                    esc_key(&self.atoms[*key as usize])
                )
            }
            Cons::AddsKeys => format!("addskeys {n}"),
            Cons::Miss { key } => format!("miss {n} {}", self.key_name(*key)),
            Cons::DefineKey { key, data, reason } => {
                format!(
                    "definekey {n} {} {} {reason}",
                    self.key_name(Key::Named(*key)),
                    *data as u8
                )
            }
            Cons::DefineAll {
                o,
                descs,
                value,
                site,
                reason,
            } => format!("defineall {n} {o} {descs} {value} {site} {reason}"),
            Cons::DefineDesc {
                o,
                key,
                site,
                reason,
            } => format!(
                "definedesc {n} {o} {} {site} {reason}",
                self.key_name(Key::Named(*key))
            ),
            Cons::Implicit { reason, proto } => {
                format!("implicit {n} {reason} {}", proto.name())
            }
            Cons::HasInstance { reason, arg } => format!("hasinstance {n} {reason} {arg}"),
            Cons::CreateProto { reason } => format!("createproto {n} {reason}"),
            Cons::ChainUnion {
                key,
                dst,
                of,
                builtin,
            } => {
                format!(
                    "chainunion {n} {} {dst} {of} {builtin}",
                    self.key_name(*key)
                )
            }
            Cons::ProtoGiven { protos } => format!("protogiven {n} {protos}"),
            Cons::ParentGiven { of } => format!("parentgiven {n} {of}"),
            Cons::ProtoClosure { dst } => format!("protoclosure {n} {dst}"),
            Cons::ProtoMethods { origin, key, call } => {
                format!("protomethods {n} {origin} {} {call}", self.key_name(*key))
            }
            Cons::AnyAccessors { origin, act } => format!("anyaccessors {n} {origin} {act:?}"),
            Cons::UnnamedSet { target } => format!("unnamedset {n} {target}"),
            Cons::StoreTopProto { origin, src } => format!("storetopproto {n} {origin} {src}"),
            Cons::Super { start, key, act } => {
                format!("super {n} {start} {} {act:?}", self.key_name(*key))
            }
            Cons::SuperFrom { recv, key, act } => {
                format!("superfrom {n} {recv} {} {act:?}", self.key_name(*key))
            }
            Cons::SuperCtor { class, call } => format!("superctor {n} {class} {call}"),
            Cons::SuperCtorParent {
                class,
                call,
                origin,
            } => format!("superctorparent {n} {class} {call} {origin}"),
            Cons::ThisFrom { recv } => format!("thisfrom {n} {recv}"),
            Cons::Accessor {
                holder,
                key,
                setter,
            } => {
                format!(
                    "accessor {n} {holder} {} {}",
                    self.key_name(Key::Named(*key)),
                    *setter as u8
                )
            }
            Cons::AnnexBDefine { call, setter } => format!("annexbdefine {n} {call} {setter}"),
            Cons::AnnexBLookup { call, setter } => format!("annexblookup {n} {call} {setter}"),
            Cons::AccessorInstall { key, f, setter, .. } => {
                format!(
                    "accinstall {n} {} {} {}",
                    self.key_name(Key::Named(*key)),
                    self.tok_fn(*f),
                    *setter as u8
                )
            }
            Cons::ProtoFrom { src } => format!("protofrom {n} {src}"),
            Cons::BirthField { key, src } => {
                format!("birthfield {n} {} {src}", self.key_name(*key))
            }
            Cons::ProtoOf { dst } => format!("protoof {n} {dst}"),
            Cons::ProtoValues { dst } => format!("protovalues {n} {dst}"),
            Cons::PlainKey { origin, act } => format!("plainkey {n} {origin} {act:?}"),
            Cons::IterNext { dst } => format!("iternext {n} {dst}"),
            Cons::NewTargetHard { reason } => format!("newtargethard {n} {reason}"),
            Cons::NativeParent {
                tm,
                expected,
                reason,
            } => format!("nativeparent {n} {tm} {expected} {reason}"),
            Cons::ShareBuffer { ta } => format!("sharebuffer {n} {ta}"),
            Cons::Captures { rec, pat } => format!("captures {n} {rec} {pat}"),
            Cons::ObjTagProbe { t, key } => format!("objtagprobe {n} {t} {key}"),
            Cons::StrReplace { dst, .. } => format!("strreplace {n} {dst}"),
            Cons::StrIndex { dst, .. } => format!("strindex {n} {dst}"),
            Cons::StrSubstring { dst, .. } => format!("strsubstring {n} {dst}"),
            Cons::ClassReplace { dst, .. } => format!("classreplace {n} {dst}"),
            Cons::RegExpPattern { tok } => format!("regexppattern {n} {tok}"),
            Cons::ViewedBuffer { view, fresh, share } => {
                format!("viewedbuffer {n} {view} {fresh:?} {share}")
            }
            Cons::BoundCall { reason, bound } => format!("boundcall {n} {reason} {bound}"),
            Cons::Arith { dst, .. } => format!("arith {n} {dst}"),
            Cons::Range {
                op,
                other,
                dst,
                right,
            } => format!("range {n} {} {other} {dst} {}", op.name(), *right as u8),
            Cons::Plus { str, num, .. } => format!("plus {n} {str} {num}"),
            Cons::ArrayLenArg { dst } => format!("arraylenarg {n} {dst}"),
            Cons::CaseMap { dst, upper } => format!("casemap {n} {dst} {upper}"),
            Cons::ConsoleFormat { reason, agg } => format!("consolefmt {n} {reason} {agg}"),
            Cons::IdxLoad { idx, dst, .. } => format!("idxload {n} {idx} {dst}"),
            Cons::EnumChain { dst, of, .. } => format!("enumchain {n} {of} {dst}"),
            Cons::EnumLoad { dst, .. } => format!("enumload {n} {dst}"),
            Cons::ForInAcc { probe, .. } => format!("forinacc {n} {probe}"),
            Cons::OwnDescAcc { dst, .. } => format!("owndescacc {n} {dst}"),
            Cons::ForInMeet { other, gate, .. } => format!("forinmeet {n} {other} {gate}"),
            Cons::IdxKey { obj, dst, .. } => format!("idxkey {n} {obj} {dst}"),
            Cons::ElemUndef { obj, key, dst } => format!("elemundef {n} {obj} {key} {dst}"),
            Cons::IdxOwnLoad { idx, dst, .. } => format!("idxownload {n} {idx} {dst}"),
            Cons::IdxOwnKey { obj, dst, .. } => format!("idxownkey {n} {obj} {dst}"),
            Cons::OwnTest { dst, at } => format!("owntest {n} {dst} {at:?}"),
            Cons::OwnGate { obj, idx, dst, .. } => {
                format!("owngate {n} {obj} {idx} {dst}")
            }
            Cons::IdxMCall { idx, call, .. } => format!("idxmcall {n} {idx} {call}"),
            Cons::IdxKeyCall {
                obj, prim, call, ..
            } => format!("idxkeycall {n} {obj} {prim} {call}"),
            Cons::ForInBody { obj, call } => format!("forinbody {n} {obj} {call}"),
            Cons::ForInKeys { ft, call } => format!("forinkeys {n} {ft} {call}"),
            Cons::DefineKeyed {
                o,
                reason,
                accessor,
            } => format!("definekeyed {n} {o} {reason} {}", *accessor as u8),
            Cons::StoreKeyed { o, src, site, subj } => {
                format!("storekeyed {n} {o} {src} {site} {subj}")
            }
            Cons::EnumKeyStore { o, src, from, .. } => {
                format!("enumkeystore {n} {o} {src} {from:?}")
            }
            Cons::EnumKeyAny { o, src, .. } => format!("enumkeyany {n} {o} {src}"),
            Cons::StoreIndexed { o, src, site, .. } => format!("storeindexed {n} {o} {src} {site}"),
            Cons::DefineAnyKey {
                o,
                key,
                value,
                get,
                set,
                site,
                reason,
                accessor,
            } => format!(
                "defineanykey {n} {o} {key} {value} {get} {set} {site} {reason} {}",
                *accessor as u8
            ),
            Cons::AccessorKeyed { holder, f, setter } => {
                format!("accessorkeyed {n} {holder} {f} {}", *setter as u8)
            }
            Cons::ThisTo { origin } => format!("thisto {n} {origin}"),
            Cons::CallArgs { call } => format!("callargs {n} {call}"),
            Cons::RetInto { call, run } => format!("retinto {n} {call} {run}"),
            Cons::ArgInto { call, run, i } => format!("arginto {n} {call} {run} {i}"),
            Cons::Spread { into, site } => format!("spread {n} {into} {site}"),
            Cons::SpreadFrom { src, site, .. } => format!("spreadfrom {n} {src} {site}"),
            Cons::OwnComputedKey { key, val } => format!("owncomputedkey {n} {key} {val}"),
            Cons::ProtoKeySet { target, val, site } => {
                format!("protokeyset {n} {target} {val} {site}")
            }
            Cons::ThisInst { call, ft } => format!("thisinst {n} {call} {ft}"),
            Cons::NativeThis { call, f } => format!("nativethis {n} {call} {f}"),
            Cons::BuiltinCallee => format!("builtincallee {n}"),
            Cons::HolderStep { key, pass, stop } => {
                format!("holderstep {n} {} {pass} {stop}", self.key_name(*key))
            }
            Cons::VisitHolders { origin, key, act } => {
                format!("visitholders {n} {origin} {} {act:?}", self.key_name(*key))
            }
            Cons::VisitStop { origin, key, act } => {
                format!("visitstop {n} {origin} {} {act:?}", self.key_name(*key))
            }
        }
    }

    /// One word for a token's kind (diagnostics).
    pub fn tok_kind_word(&self, t: Tok) -> &'static str {
        match &self.toks[t as usize] {
            TokKind::Obj(tm, _) => match self.tmpls[*tm as usize].kind {
                TmplKind::Class { .. } => "class instance",
                TmplKind::Lit { .. } => "literal",
                TmplKind::FnInst { .. } => "fn instance",
                TmplKind::Arr { .. } => "array",
                TmplKind::Args { .. } => "arguments object",
                TmplKind::ObjCreate { .. } => "Object.create object",
                TmplKind::BObj { .. } => "builtin object",
            },
            TokKind::Func(_, _) => "function",
            TokKind::Bound(_) => "bound function",
            TokKind::ClassObj(_) => "class",
            TokKind::Proto(_) => "prototype",
            TokKind::ModNs(_) => "module namespace",
            _ => "value",
        }
    }

    pub fn fn_tokens(&self, n: NodeId) -> Vec<Tok> {
        self.fns_of(n)
    }
}
