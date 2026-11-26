//! Whole-program invariance facts (wpfacts), slice 1.
//!
//! An inclusion-based (Andersen-style) value-flow analysis over the final
//! HIR of every module, failing closed: anything it does not model flows to
//! one ⊤ node. It computes, per birth template (class, object-literal
//! shape, constructor-function body):
//!
//! - F1 writers / readonly-after-construction fields,
//! - F1′ definite birth keys,
//! - F2 field value kinds,
//! - F3 escape (E1–E25, B1–B4), prefix-stable and fixed-layout,
//! - F4 receiver/parameter class sets (as site verdicts),
//! - F5 single method targets.
//!
//! Slice 1 removes no check: codegen consumes only the per-module digest
//! (object-cache key) and, in contract builds, the claims section. See
//! `secret-tests/scratchpad/wpfacts-design-20261012.md`.

pub mod bindings;
mod booleans;
mod born_with;
mod k3_census;
mod builtins;
pub mod call_kinds;
pub mod canonical_numbers;
mod const_arrays;
mod const_numbers;
mod ctor_phase;
mod decl_units;
pub mod delta;
mod demand;
pub mod derive;
mod diagnostics;
mod dump;
mod element_floors;
mod element_keys;
pub mod family_reach;
pub mod field_read_calls;
mod fill_births;
mod gen;
mod gen_call;
mod gen_cjs;
mod gen_const;
mod gen_expr;
mod gen_protocol;
mod gen_rest;
mod gen_sources;
mod held_lengths;
mod hold_ledger;
mod ledger;
pub mod list_caps;
mod literal_strings;
mod member_ops;
mod method_sites;
pub mod model;
mod named_lengths;
pub mod native_effects;
mod provenance;
pub mod pts;
mod push_fills;
mod quiet_sites;
mod reach;
pub mod records;
mod rest_elems;
mod rest_fields;
mod rooted;
pub mod signatures;
mod solve;
mod summary;
#[cfg(test)]
mod tests;
pub mod topstop;
mod units;
mod unksrc;
mod zero_sign;

pub use bindings::{
    ArrRep, BindingFacts, BodyRef, ElemRep, ElementRead, Enc, ModuleBindings, RecRep, Rep,
};
pub use derive::dispatch::{elide_enabled, MethodCallCertificate};
pub use derive::native_calls::native_member_calls_of;
pub use derive::{
    Access, DispatchObligation, ElementFacts, ElementKey, ElementOp, FieldFacts, KindFact,
    ReceiverValue, SiteFacts, TemplateFacts, Verdict,
};
pub use family_reach::{FamilyReach, FamilyVerdict, NativeDispatch};
pub use field_read_calls::{FieldReadCall, FieldReadDefault};
pub use member_ops::{MemberForm, MemberOpFacts};
use perry_hir::Module;
pub use signatures::Signature;

/// Reserved atom for computed-key accessors ("any key").
pub(crate) const STAR_KEY: &str = "\u{1}*";

/// The JS kind of a native-library extern's result as perry's lowering makes
/// it from the manifest descriptor (perry-codegen `lower_call/extern_func.rs`):
/// `void` is `undefined`, `string` / `i64_str` a string, `bool` a boolean,
/// every numeric descriptor a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeResult {
    Number,
    String,
    Boolean,
    Undefined,
}

#[derive(Clone, Debug, Default)]
pub struct FactsConfig {
    /// Export body classifications for the compiler diagnostic mode only.
    pub verify_facts: bool,
    /// Export the classified bodies for dead-body removal (`PERRY_FACTS_DCE=1`,
    /// off by default).
    pub dce_bodies: bool,
    /// Modules the program loads besides the ones no edge enters (worker
    /// entries, bundle extensions): their bodies run whatever imports them.
    pub roots: Vec<String>,
    /// Capture observational site data for the existing facts dump only.
    pub diagnostics: bool,
    /// Record the constraint relations for the independent recomputation.
    pub relations: bool,
    /// No integer intervals (`model::range`): every Number is ⊤
    /// (`PERRY_FACTS_INT_RANGES=0`).
    pub no_int_ranges: bool,
    /// No array length intervals: every Array `length` is ⊤
    /// (`PERRY_FACTS_ARRAY_LENGTHS=0`).
    pub no_array_lengths: bool,
    /// No length floor: growth makes an Array's `length` ⊤ instead of
    /// raising its upper bound (`solve::arr_len`; `PERRY_FACTS_ARRAY_FLOOR=0`).
    pub no_array_floor: bool,
    /// No plain Array births, fused-fill birth or `Rep::Arr`
    /// (`PERRY_FACTS_ARRAY_REPS=0`, DESIGN-p4l1 PR-B).
    pub no_array_reps: bool,
    /// No immutable Arrays and no constant contents
    /// (`PERRY_FACTS_CONST_ARRAYS=0`, `const_arrays`).
    pub no_const_arrays: bool,
    /// Value records (`PERRY_FACTS_VALUE_RECORDS=1`; off unless
    /// set until VR-6): identity observations become `Cons::Identity`.
    pub value_records: bool,
    /// No record signature slots (`PERRY_FACTS_RECORD_SIGNATURES=0`,
    /// VR-3b): a record in a typed call's argument or result
    /// rejects its template.
    pub no_record_signatures: bool,
    /// MATHB1: the namespace pairs are the knob's (`PERRY_NS_PAIRS=1`, perry-hir
    /// `PatchedBuiltins::namespace_pairs`): each is walked as one call of the
    /// member (`Model::ns_pairs`). Off, the pairs a program has are main's
    /// shadow-bit guards, walked as before.
    pub namespace_pairs: bool,
    /// No `Rep::Arr` for a field and no in-floor field read call
    /// (`PERRY_FACTS_ARRAY_FIELDS=0`, DESIGN-p4l1 PR-C3).
    pub no_array_fields: bool,
    /// R1: a `Cons::Range` result widens like any node again, after
    /// `WIDEN_STEPS` growths (`PERRY_FACTS_EXACT_RULES=0`;
    /// `solve::grow_rule_range`).
    pub no_exact_rules: bool,
    /// −0 tolerance off (`PERRY_FACTS_ZERO_SIGN=0`, PR D, `zero_sign`): the
    /// strict interval rules, a possible `-0` makes the result ⊤.
    pub no_zero_sign: bool,
    /// R1: a write of a narrowed local in a branch that always leaves ends
    /// the narrowed reads after the branch again (`PERRY_FACTS_LEAVING_WRITES=0`;
    /// `gen::narrow`, "Writes on a leaving path").
    pub no_leaving_writes: bool,
    /// An element read's past-the-end `undefined` is unconditional again
    /// (`PERRY_FACTS_ELEM_UNDEF=0`, PR-C1: `Cons::ElemUndef`).
    pub no_elem_undef: bool,
    /// N6 off (`PERRY_FACTS_CONST_NUMBERS=0`): bitwise operators keep their
    /// int32 hull and no module binding is exported as a constant Number
    /// (`const_numbers`).
    pub no_const_numbers: bool,
    /// No guard narrowing, predicate summaries or String code intervals
    /// (`gen::narrow` guards; `PERRY_FACTS_GUARD_RANGES=0`).
    pub no_guard_ranges: bool,
    /// No presence narrowing: a truthy or non-nullish guard leaves the
    /// local's `null`/`undefined` (`gen::narrow` guards;
    /// `PERRY_FACTS_GUARD_PRESENCE=0`).
    pub no_guard_presence: bool,
    /// OPTIMISTIC rule, default off, measurement only (owner 2026-11-12;
    /// `PERRY_FACTS_OPTIMISTIC_JSON_KEY=1`): `Model::json_key_optimistic`.
    pub optimistic_json_key: bool,
    /// ASSUMED (owner 2026-11-16, "Assume it for now", interim until an
    /// exact link-time check of the library's imports): a native-library
    /// extern whose manifest signature (`perry.nativeLibrary.functions`) has
    /// only primitive parameters and result reaches no program object. By
    /// symbol, the externs the driver found so, with their result kind
    /// (`Gen::native_extern_call`); empty: every extern call is unknown code.
    pub native_externs: std::collections::BTreeMap<String, NativeResult>,
    /// JSON position classes (`solve::json_pos`), default OFF
    /// (`PERRY_FACTS_JSON_POSITIONS=1`): off, a parse's objects are one
    /// token (BATCHB28: on, the three.js skin solve runs past its bound).
    pub json_positions: bool,
    /// Test-only negative control: plant a wrong fact
    /// (`readonly:T.f`, `prefix_stable:T`, `fixed_layout:T`,
    /// `kind:T.f=num|str|bool|obj`, `definite:T.f`).
    pub sabotage: Option<String>,
    /// Wall-clock budget (seconds) on generation and solve; past it the
    /// program gets no facts.
    pub budget_secs: Option<u64>,
    /// Per-constraint-kind time profile in the report.
    pub profile: bool,
    /// Bound on points-to entries (work/memory); past it, no facts.
    pub max_pts: Option<u64>,
    /// Deterministic solve-work bound (token propagations along edges and
    /// into constraints); past it, no facts. Unlike the wall budget it
    /// gives the same answer on every machine.
    pub max_work: Option<u64>,
    /// The driver knows the program can run interpreted code (a deferred
    /// `new Function`/`eval` site or `node:vm`). Not a kill (decision 150):
    /// interpreted code is the ⊤ summary node; see
    /// [`ProgramFacts::interp_guard`].
    pub dynamic_code: Option<String>,
    /// DECISIONS 235: the program holds a real eval-like source site (a
    /// deferred eval/Function source, `node:vm`, a dynamic import of a
    /// computed data URL): interpreted code may run from the start. Without
    /// one, it runs only once the solve finds a call or construct that may
    /// be the Function constructor (`builtins::interpreted_code_runs`). The
    /// link decision (`dynamic_code`) does not feed the facts.
    pub interpreted_sources: bool,
    pub dynamic_code_reach: perry_hir::dynamic_code::DynamicCodeReach,
    /// Diagnostics: name where ⊤ came from in each ⊤ site verdict.
    pub explain: bool,
    /// The driver's resolution of every `export … from "src"` source: a
    /// re-export has no import of its source to resolve it by.
    pub export_sources: ExportSources,
    /// The modules are one unit of a larger program: every other unit is
    /// unknown compiled code (`Gen::unit_boundary`).
    pub unit: bool,
    /// Decision 32: threads for the per-unit driver's dependency levels
    /// (0 and 1 solve on the calling thread). The facts are the same at
    /// every count.
    pub jobs: usize,
    /// Per-unit inputs/outputs digests in the dump (`# uio` / `# uout`):
    /// what an edit changes, unit by unit (design §13.3).
    pub unit_io: bool,
    /// The per-unit driver prints a census line per dependency level and
    /// per round (`PERRY_FACTS_REPORT=1`): what each holds, and the RSS.
    pub report: bool,
    /// Dump only (per-unit driver): the file the units' diagnostic records are
    /// written to as each unit is captured at the fixpoint, instead of being
    /// held for the dump (`ProgramFacts::diag_stream`).
    pub diag_sidecar: Option<std::path::PathBuf>,
    /// Dump only (with `diagnostics`): the birth census records (`birth-*`
    /// lines: arrivals, escapes, the reads and writes of intrinsic keys and
    /// the receiver of a missed intrinsic method call). The compiler sets it
    /// from `PERRY_FACTS_BIRTH_CENSUS`. Facts do not depend on it.
    pub birth_census: bool,
}

/// Module path → `export … from` source → the module path it resolves to.
pub type ExportSources =
    std::collections::HashMap<String, std::collections::HashMap<String, String>>;

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub modules: usize,
    pub templates: usize,
    pub fact_units: usize,
    pub nodes: usize,
    pub constraints: u64,
    pub tokens: usize,
    pub functions: usize,
    pub gen_ms: f64,
    pub solve_ms: f64,
    pub derive_ms: f64,
    pub pops: u64,
    /// Summary instances, the shared ones among them, and calls expanded
    /// into their caller's summary.
    pub instances: u64,
    pub shared_instances: u64,
    pub composed: u64,
    /// Calls the solver made (a builtin calling back, a bound target, a
    /// protocol step: `Model::solver_call`).
    pub solver_calls: u64,
    /// Closure contexts made (design §14.2).
    pub contexts: u64,
    /// Solve work done (see `FactsConfig::max_work`).
    pub work: u64,
    /// The solve hit its budget: no facts were claimed.
    pub aborted: bool,
    /// The solve passed its work bound and finished widened
    /// (`Model::widened`): the facts no later arrival touched.
    pub widened: bool,
    /// The widened solve stopped at `WIDENED_WORK` times the bound
    /// (`aborted` is set too: no facts).
    pub widened_stop: bool,
    /// Contract checks run after the solve (§11.2): none past the bound.
    pub checks: u64,
    pub top_calls: u64,
    /// Rule G2 filters (`Cons::FieldTruthy`): rules, and the rules whose
    /// result left out at least one object token of their host.
    pub field_truthy: (u64, u64),
    /// Dump only: `# ftr` rows (`field_truthy_rows`).
    pub field_truthy_rows: Vec<String>,
    /// Dump only: solve counters (`solve_counters`).
    pub solve_counters: Vec<String>,
    /// (constraint kind, applications, ms), most expensive first.
    pub profile: Vec<(String, u64, f64)>,
    pub pts_entries: u64,
    /// The largest points-to sets: (node label, size, token-kind mix).
    pub big_nodes: Vec<(String, usize, String)>,
    /// Rule R1: how many program fields of built-in objects, the largest
    /// first (`holder.key`, set size), the sinks (`Model::r1_sinks`).
    pub builtin_fields: (usize, Vec<(String, usize)>, [u64; 4]),
}

/// Stable body identity at the canonical HIR boundary: canonical module
/// path, HIR function/body id, and the existing synthetic-context name.
/// Ordinary bodies use an empty name; synthetic holding contexts preserve
/// their existing name identity. No model node, frame or instance id enters
/// this identity. Module indices are remapped to paths when exporting it.
pub type BodyIdentity = (String, u32, String);

/// The single exported reach projection: admitted/walked bodies together
/// with synthetic contexts holding sites, produced by existing activation.
/// An interrupted solve (including any bound or early stop) is incomplete.
/// Consumers must treat an incomplete projection as everything active.
/// Absence is useful only for actual bodies of the same canonical HIR;
/// unknown or newly minted identities never establish inactivity.
#[derive(Clone, Debug, Default)]
pub struct BodyReach {
    pub bodies: std::collections::BTreeSet<BodyIdentity>,
    /// Every body the activation pass classified (active or not). Only an
    /// identity in this set can be called inactive; a missing or new
    /// identity is active.
    pub seen: std::collections::BTreeSet<BodyIdentity>,
    pub complete: bool,
}

impl BodyReach {
    /// The one inactive predicate (the complement of the activate
    /// predicate): the solve completed, the pass classified this body and
    /// no path reaches it. Dead-body removal and the claim check read
    /// only this.
    pub fn inactive(&self, id: &BodyIdentity) -> bool {
        self.complete && self.seen.contains(id) && !self.bodies.contains(id)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProgramFacts {
    /// Dump-only JSON records, scoped by emitting analysis-unit identity.
    pub diagnostics: Vec<(String, Vec<String>)>,
    /// Dump-only: where the per-unit diagnostic records went when they were
    /// streamed (`FactsConfig::diag_sidecar`): the file and, in unit order,
    /// each unit's (offset, length).
    pub diag_stream: Option<(std::path::PathBuf, Vec<(u64, u64)>)>,
    /// Dump-only: the addresses of the HIR sites the analyses recorded
    /// (`ledger::census` compares them with the HIR).
    pub(crate) diagnostic_sites: Vec<(usize, String, &'static str, u32)>,
    /// DECISIONS 225: the activation pass's count (the whole program).
    pub activation: Option<String>,
    /// DECISIONS 225: escaped functions of the final solve whose body was
    /// not walked (`Model::escaped_inactive`); must be 0.
    pub escaped_inactive: Option<usize>,
    /// Diagnostic-only export of the activation pass's seen body classifications.
    pub verification_bodies: Option<std::collections::BTreeMap<(String, u32, String), bool>>,
    /// Completed whole-program reach, or an incomplete conservative view
    /// when activation was unavailable or the solve stopped early.
    pub body_reach: BodyReach,
    /// Per runtime family, whether the program may ever create or observe a
    /// value of it (`family_reach`). `None`: not computed
    /// (`PERRY_FACTS_FAMILY_REACH=0`, a per-unit run): every family is live.
    /// Nothing reads it yet but the dump.
    pub family_reach: Option<FamilyReach>,
    /// DESIGN-native-dispatch PR 1: whether every route into the runtime's
    /// generic method dispatcher is a member-call site stating its
    /// obligation (`family_reach::NativeDispatch`). `None` exactly where
    /// `family_reach` is: every stage is live. Nothing reads it but the
    /// dump.
    pub native_dispatch: Option<NativeDispatch>,
    /// NDISP2: the site record of every HIR-specialized member op
    /// (`member_ops`): its dispatcher obligation. Empty where
    /// `native_dispatch` is `None`.
    pub member_ops: Vec<MemberOpFacts>,
    pub module_paths: Vec<String>,
    pub templates: Vec<TemplateFacts>,
    /// The Arr templates linked to a record template (R6), for
    /// the `records` ledger.
    pub record_arrays: Vec<records::RecordArray>,
    pub sites: Vec<SiteFacts>,
    /// Computed member sites in a whole-program solve. Per-unit facts do
    /// not publish values whose token/template ids belong to separate solves.
    pub elements: Vec<ElementFacts>,
    pub stats: Stats,
    /// Interpreted code can run (decision 150), and why. It is no kill: its
    /// data flow is the ⊤ summary node (everything it can receive already
    /// escapes there). What the analysis cannot see is interpreted code
    /// writing an intrinsic prototype (an accessor or method under any key
    /// on `Object.prototype`, `Array.prototype` …): that runs it with the
    /// receiver of any lookup reaching that prototype, and a setter there
    /// keeps a constructor's assignment from defining its birth field. The
    /// runtime records exactly that (`PERRY_INTERP_PROTO_WRITTEN`, set-only),
    /// so with this set every proof is GUARDED: the site tests the word and
    /// takes its complete generic operation once it is set.
    pub interp_guard: Option<String>,
    /// Compiled code may write `%Function.prototype%.bind` (or the
    /// `name`/`length` bind reads), and why; `None`: it is the builtin's own
    /// data property for the whole run as far as compiled code goes (an
    /// aborted solve answers `Some`). Interpreted code is
    /// [`ProgramFacts::interp_guard`]'s.
    pub fnproto_bind_written: Option<String>,
    /// ASSUMED (`FactsConfig::native_externs`): the extern call sites
    /// (module path, the call's byte offset; `None`: a spread call) modeled
    /// by the native-extern assumption; every fact of the program rests on
    /// it when this is non-empty.
    pub native_extern_sites: Vec<(String, Option<u32>)>,
    /// #13645: an array's `@@iterator` may not be the builtin, and why
    /// (`Model::array_iteration_writes`); `None`: compiled code never
    /// changes it. Interpreted code is [`ProgramFacts::interp_guard`]'s.
    pub array_iteration_written: Option<String>,
    /// #13645: `%ArrayIteratorPrototype%`'s chain may gain or change `next`
    /// or `return`, and why; `None`: compiled code never does.
    pub array_iterator_close_written: Option<String>,
    /// Constructors whose live [[Prototype]] cannot be relinked by any reachable operation,
    /// with the class their evaluated `extends` value is (DEC 251 B; `None`: no class parent,
    /// a base class or a native one). A class whose `extends` value settle could not link
    /// to one class is absent: its construction follows the live superclass.
    pub super_unrelinked: std::collections::BTreeMap<u32, Option<u32>>,
    /// Compiled code may replace Function.prototype.call/apply; fail closed.
    pub fnproto_borrow_written: Option<String>,
    pub dynamic_code_reach: perry_hir::dynamic_code::DynamicCodeReach,
    pub sabotage: Option<String>,
    pub relations: Option<Vec<String>>,
    /// The dump's `# rep` probe lines (`bindings::rep_probe`), with the
    /// dump only.
    pub rep_probe: Vec<String>,
    /// −0 tolerance (PR D, `zero_sign`): Number nodes that keep an integer
    /// interval although they may hold `-0`, which no use can observe.
    /// Check mode's int32 witnesses admit `-0` when it is nonzero (a
    /// tolerated value computed as a double is `-0`, and that is no false
    /// proof); the sign witness is codegen's.
    pub zero_sign_tolerated: u32,
    pub tok_names: Vec<String>,
    /// Receiver facts per binding, per module (slice 2: codegen elision).
    pub bindings: Vec<ModuleBindings>,
    /// No solved native-prototype/global write in any contributing unit.
    pub indexed_metadata_unwritten: bool,
    module_digests: Vec<u64>,
}

/// Run the analysis over every module (paths must be the keys the caller
/// uses for `digest_for` / `contract_section`).
pub fn compute(modules: &[(String, &Module)], cfg: &FactsConfig) -> ProgramFacts {
    compute_with(modules, cfg, None)
}

/// [`compute`] over the bodies `given` says can run (the compile's one
/// activation pass, [`Activation`]); `None`: the pass runs here.
pub fn compute_with(
    modules: &[(String, &Module)],
    cfg: &FactsConfig,
    given: Option<Activation>,
) -> ProgramFacts {
    let mut pf = compute_act(modules, cfg, None, None, Some(given));
    if cfg.diagnostics {
        let mut sorted: Vec<(&str, &Module)> =
            modules.iter().map(|(p, m)| (p.as_str(), *m)).collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        pf.diagnostics.push((
            "<census>".into(),
            ledger::census(&sorted, &pf.diagnostic_sites),
        ));
    }
    pf
}

#[path = "lib/program_facts.rs"]
mod program_facts;
use program_facts::{
    big_nodes, builtin_fields, field_truthy_filters, field_truthy_rows, solve_counters,
};

/// When the analysis holds the realm (`Model::restart_on_hold`).
#[derive(Clone, PartialEq, Eq)]
enum Hold {
    /// At the first hold, wherever the solve meets it (the activation pass).
    Midway,
    /// The first hold met in generation or the solve stops the analysis,
    /// which then runs again `Seeded`.
    Once,
    /// Held from the seed, for the first hold's unknown code when its site
    /// named it (`Model::hold_why`).
    Seeded(Option<String>),
}

/// Generation and the summaries for one analysis of `sorted` (with `act`,
/// DECISIONS 225: only the bodies that can run are walked; an unfixed set
/// is the activation pass, which walks each body once it is activated).
fn generate<'a>(
    sorted: &[(&'a str, &'a Module)],
    cfg: &FactsConfig,
    published: Option<&'a units::Published>,
    view: Option<&'a decl_units::UnitView>,
    act: Option<model::Act>,
    hold: Hold,
    t0: std::time::Instant,
) -> (gen::Gen<'a>, f64, std::time::Instant) {
    let loud = act.as_ref().is_none_or(|a| a.fixed || a.inert);
    // Node IDs name the final observed model. Never apply its diagnostic
    // suppression selector to intermediate liveness models with other IDs.
    let birth_census = cfg.diagnostics && loud && cfg.birth_census;
    let mut g = gen::Gen::new_observed(
        sorted.to_vec(),
        cfg.relations && loud,
        cfg.explain && loud,
        birth_census,
    );
    // DECISIONS 225: a body is walked once its function can run.
    g.m.act = act;
    match hold {
        Hold::Once => g.m.restart_on_hold = !cfg.unit && loud && g.m.prov.is_none(),
        Hold::Seeded(why) => {
            let r = match why {
                Some(w) => g.m.reason(&format!(
                    "an intrinsic of the realm: unknown code holds the realm ({w})"
                )),
                None => {
                    g.m.reason("an intrinsic of the realm: unknown code holds the realm")
                }
            };
            g.m.hold_realm(r);
        }
        Hold::Midway => {}
    }
    if cfg.diagnostics && loud {
        g.diagnostic = Some(Default::default());
        g.m.diagnostic.get_or_insert_with(Default::default);
        if cfg.birth_census && g.m.diagnostic.as_ref().unwrap().census.is_none() {
            g.m.diagnostic.as_mut().unwrap().census = Some(crate::diagnostics::BirthCensus {
                suppress: (std::env::var("PERRY_FACTS_ONLY").ok().as_deref() == Some("1"))
                    .then(|| std::env::var("PERRY_FACTS_SUPPRESS_BIRTH").ok())
                    .flatten(),
                ..Default::default()
            });
        }
    }
    g.export_sources = cfg.export_sources.clone();
    g.roots = cfg.roots.clone();
    g.dynimport_sabotage = cfg.sabotage.as_deref() == Some("dynimport");
    g.m.range_sabotage = cfg.sabotage.as_deref() == Some("range");
    g.m.arr_len_sabotage = cfg.sabotage.as_deref() == Some("arrlen");
    g.m.elem_undef = !cfg.no_elem_undef;
    g.m.elem_undef_sabotage = cfg.sabotage.as_deref() == Some("arrundef");
    g.m.number_call_sabotage = cfg.sabotage.as_deref() == Some("numbercall");
    g.m.const_numbers = !cfg.no_const_numbers;
    g.m.const_number_sabotage = cfg.sabotage.as_deref() == Some("constnum");
    g.m.arr_cap_sabotage = matches!(cfg.sabotage.as_deref(), Some("arrcap" | "densebirth"));
    g.m.arr_floor_sabotage = cfg.sabotage.as_deref() == Some("arrfloor");
    g.m.arr_append_sabotage = cfg.sabotage.as_deref() == Some("arrappend");
    g.m.arr_fwd_sabotage = cfg.sabotage.as_deref() == Some("arrfwd");
    g.m.arr_floor_off = cfg.no_array_floor;
    g.m.array_reps = !cfg.no_array_reps;
    g.m.arr_dense_sabotage = cfg.sabotage.as_deref() == Some("arrdense");
    g.m.arr_elem_sabotage = cfg.sabotage.as_deref() == Some("arrelem");
    g.m.arr_lazy_sabotage = cfg.sabotage.as_deref() == Some("arrlazy");
    g.m.arr_front_sabotage = cfg.sabotage.as_deref() == Some("arrfront");
    g.m.const_arrays = g.m.array_reps && !cfg.no_const_arrays;
    g.m.arr_const_sabotage = cfg.sabotage.as_deref() == Some("arrconst");
    g.m.value_records = cfg.value_records;
    g.m.ns_pairs = cfg.namespace_pairs;
    g.m.record_signatures = cfg.value_records && !cfg.no_record_signatures;
    g.m.array_fields = !cfg.no_array_fields;
    g.m.k3_held = std::env::var_os("PERRY_K3_CENSUS").map(|_| Default::default());
    g.m.rfam.explain_off = std::env::var_os("PERRY_K3_FAMOFF").is_some();
    g.m.arr_field_sabotage = cfg.sabotage.as_deref() == Some("arrfield");
    g.m.rest_field_sabotage = cfg.sabotage.as_deref() == Some("restfield");
    g.m.rest_elems = rest_elems::enabled();
    g.m.rest_capacity = rest_elems::capacity_enabled();
    g.m.fill_births = fill_births::enabled();
    g.m.push_fills = push_fills::enabled();
    g.m.push_fill_sabotage = cfg.sabotage.as_deref() == Some("restpush");
    g.m.rest_elem_sabotage = cfg.sabotage.as_deref() == Some("restelem");
    g.m.rest_fields = rest_fields::enabled();
    g.m.key_sign = element_keys::sign_enabled();
    g.m.key_sign_sabotage = cfg.sabotage.as_deref() == Some("keysign");
    g.m.wide_keys = element_keys::wide_enabled();
    g.m.slice_births = builtins::slice_births_enabled();
    g.m.inbounds_store = gen::in_bounds::enabled();
    g.m.inbounds_sabotage = cfg.sabotage.as_deref() == Some("inbounds");
    gen::in_bounds::configure_read(&mut g.m, cfg.sabotage.as_deref());
    g.m.no_hole_sabotage = cfg.sabotage.as_deref() == Some("nohole");
    g.m.slice_birth_sabotage = cfg.sabotage.as_deref() == Some("slicebirth");
    g.m.wide_key_sabotage = cfg.sabotage.as_deref() == Some("widekey");
    g.m.held_lengths = held_lengths::enabled() && !cfg.unit;
    g.m.instance_nodes =
        bindings::instance_nodes_enabled() && cfg.sabotage.as_deref() != Some("instlit");
    g.m.named_lengths = named_lengths::enabled() && !cfg.unit;
    g.m.map_values = solve::map_vals::enabled();
    g.m.map_values_sabotage = cfg.sabotage.as_deref() == Some("mapvals");
    g.m.literal_strings = literal_strings::enabled();
    g.m.literal_strings_sabotage = cfg.sabotage.as_deref() == Some("litstr");
    booleans::configure(&mut g.m, cfg.sabotage.as_deref());
    g.m.rest_lane_sabotage = cfg.sabotage.as_deref() == Some("restlane");
    g.m.exact_rules = !cfg.no_exact_rules;
    g.m.exact_rule_sabotage = cfg.sabotage.as_deref() == Some("exactrule");
    g.m.zero_sign =
        !cfg.no_zero_sign && !cfg.no_int_ranges && !cfg.unit && demand::at_rest_demand();
    g.m.zero_sign_sabotage = cfg.sabotage.as_deref() == Some("zerosign");
    g.leaving_writes = !cfg.no_leaving_writes;
    g.leaving_write_sabotage = cfg.sabotage.as_deref() == Some("leavingwrite");
    g.int_ranges = !cfg.no_int_ranges;
    g.arr_lengths = g.int_ranges && !cfg.no_array_lengths;
    g.guard_ranges = g.int_ranges && !cfg.no_guard_ranges;
    g.guard_presence = !cfg.no_guard_presence;
    g.m.optimistic_json_key = cfg.optimistic_json_key;
    g.m.native_externs = cfg.native_externs.clone();
    g.m.json_positions = cfg.json_positions;
    g.name_folds = reach::name_folds() && cfg.sabotage.as_deref() != Some("noreachfolds");
    g.m.reach_new_target =
        reach::new_target() && cfg.sabotage.as_deref() != Some("noreachnewtarget");
    g.m.reach_step_done = reach::step_done() && cfg.sabotage.as_deref() != Some("noreachstepdone");
    g.m.reach_gen_members =
        reach::gen_members() && cfg.sabotage.as_deref() != Some("noreachgenmembers");
    g.m.reach_thenable = reach::thenable() && cfg.sabotage.as_deref() != Some("noreachthenable");
    g.m.reach_iter_close =
        reach::iter_close() && cfg.sabotage.as_deref() != Some("noreachiterclose");
    g.m.reach_spread_args =
        reach::spread_args() && cfg.sabotage.as_deref() != Some("noreachspreadargs");
    reach::configure(&mut g.m, cfg.sabotage.as_deref());
    g.unit_in = published;
    g.view = view;
    let interp = cfg.dynamic_code.clone().unwrap_or_else(|| "-".into());
    let dynamic_reach = cfg.dynamic_code_reach.globals as u8;
    g.m.rel(|| format!("dynamicreach {dynamic_reach}"));
    g.m.rel(|| format!("interp {}", interp.replace(' ', "\\s")));
    // The activation pass profiles too (same env): its work, curve and
    // top kinds / nodes are reported when it ends or stops at a bound.
    if cfg.profile {
        g.m.profile = Some(Default::default());
        g.m.prof = Some(Default::default());
    }
    // The bounds are in force from the first constraint: generation already
    // solves online (`Model::cons` and `Model::edge` apply each rule to what
    // its node holds), so it is part of the solve they bound.
    if let Some(secs) = cfg.budget_secs {
        g.m.budget = Some((t0, std::time::Duration::from_secs(secs)));
    }
    g.m.unit_mode = cfg.unit;
    g.m.family_reach.on = !cfg.unit && family_reach::enabled();
    g.m.max_pts = cfg.max_pts.unwrap_or(0);
    g.m.max_work = cfg.max_work.unwrap_or(0);
    g.m.widen = !cfg.unit;
    g.m.max_nodes = cfg.max_pts.map(|p| (p / 8) as usize).unwrap_or(0);
    g.m.interp_free = cfg.dynamic_code.is_none()
        && !cfg.dynamic_code_reach.globals
        && cfg.dynamic_code_reach.lexical_modules.is_empty()
        && cfg.dynamic_code_reach.sites.is_empty();
    // Interpreted code reads and writes any global (E20; the prototype-write
    // guard does not cover it) and catches what compiled code throws. A real
    // source site runs it from the start; otherwise only a solved call that
    // may be the Function constructor does (DECISIONS 235). Per unit another
    // unit's solve can be that call, so the link decision stands for it.
    g.m.interp_linked = cfg.dynamic_code.is_some() || cfg.dynamic_code_reach.globals;
    if cfg.interpreted_sources
        || cfg.dynamic_code_reach.globals
        || (cfg.unit && cfg.dynamic_code.is_some())
    {
        g.m.interp_linked = true;
        builtins::interpreted_code_runs(&mut g.m);
    }
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    if cfg.unit {
        // The closed contracts first (decision 19): publishing a closed
        // function is no escape only once its contract is known.
        g.closed_params();
        g.published_receivers();
        g.published_args();
        g.published_ctors();
        g.hold_binding_writes();
        g.unit_boundary(published);
        // DECISIONS 241: this unit's contract functions read what every
        // unit stored on their own properties.
        if published.is_some_and(|p| !p.fn_stores.is_empty()) {
            let mut fs: Vec<model::FnIdx> =
                g.m.contract_fns
                    .iter()
                    .copied()
                    .filter(|&f| g.m.funcs[f as usize].func_id != u32::MAX)
                    .collect();
            fs.sort_unstable();
            for f in fs {
                let name = fn_contract_name(&g, f).0;
                g.read_fn_stores(0, f, &name);
            }
        }
        // The other units' writes of the shared objects (`model::SharedWrite`).
        if let Some(p) = published {
            for w in &p.shared {
                g.m.replay_shared(w);
            }
        }
    }
    let gen_ms = t0.elapsed().as_secs_f64() * 1000.0;
    if cfg.profile {
        let pass = if loud { "" } else { "activation pass " };
        eprintln!("perry: wpfacts {pass}after generation {}", g.m.size_line());
    }
    let t1 = std::time::Instant::now();
    g.m.log_seed_accessors();
    for s in &g.read_sites {
        let (m0, o, k, r, c) = (s.module, s.byte_offset, s.key, s.recv, s.is_call as u8);
        g.m.rel(|| format!("readsite {m0} {o} {k} {r} {c}"));
    }
    g.m.prof_mark(crate::model::NO_NODE, "generation");
    if !loud {
        // The activation pass: one body for every call of each function,
        // the sound coarser analysis, enough to say which bodies can run.
        for f in g.m.funcs.iter_mut() {
            f.no_summary = true;
        }
    }
    // An unwalked body is no evidence, even if its token reached a guard.
    let pending: Vec<_> =
        g.m.funcs
            .iter()
            .filter_map(|f| match f.own_test {
                model::OwnTestBody::Pending(n) => Some(n),
                _ => None,
            })
            .collect();
    for n in pending {
        g.m.refute_own_test(n);
    }
    // The nodes the binding facts read keep their instance slots
    // (`summary::compress`).
    let reads = bindings::binding_reads(&g);
    g.m.binding_reads = reads;
    g.m.summarize_and_replay();
    if unksrc::on() {
        let gm = &g;
        unksrc::flush(if loud { "loud" } else { "quiet" }, |f| {
            gm.m.funcs
                .get(f as usize)
                .map(|fi| {
                    let p = gm
                        .modules
                        .get(fi.module as usize)
                        .map(|(p, _)| p.to_string())
                        .unwrap_or_default();
                    format!("{}@{}", fi.name, p)
                })
                .unwrap_or_else(|| "-".into())
        });
    }
    g.m.prof_mark(crate::model::NO_NODE, "summaries");
    // Each function stands at its declaration (`this`, the result).
    if let Some(s) = g.m.stop_top.as_mut() {
        for (i, fi) in g.m.funcs.iter().enumerate() {
            let sp = g
                .modules
                .get(fi.module as usize)
                .and_then(|(_, m)| m.closure_source_text.get(&fi.func_id))
                .and_then(|x| x.source_span);
            if let Some(sp) = sp {
                s.fn_pos.insert(i as u32, (fi.module, sp.0));
            }
        }
    }
    if (cfg.profile || cfg.explain) && loud {
        eprintln!("perry: wpfacts {}", g.m.sum.stats);
    }
    (g, gen_ms, t1)
}

/// DECISIONS 225's activation pass over the whole program, run ONCE per
/// compile: dynamic reach (right after collection) reads its live `import()`
/// sites, then the facts, over the final HIR, take its set of bodies that can
/// run ([`compute_with`]). `act` is `None` past the work bound (every body can
/// run, every site may load).
///
/// The invariant this hand-over rests on: the HIR passes between dynamic
/// reach and the facts may NOT mint or renumber a function, and may NOT add a
/// call edge (or an escape) to a function the pass saw. A pass that needs to
/// must extend the Act instead (activate what it makes reachable). A minted
/// function the pass never saw is walked (`Act::seen`, sound); a removed call
/// leaves a superset (sound; segview's for-of rewrite removes iterator calls).
/// The detector is the activation-identity measurement: the one pass's active
/// count and key digest equal the facts' own pass over the final HIR.
pub struct Activation {
    act: Option<model::Act>,
    /// The module paths the pass's keys index (sorted).
    modules: Vec<String>,
    live: Option<std::collections::BTreeSet<(String, String)>>,
}

impl Activation {
    /// The dynamic `import()` / `require(expr)` specifiers a body that can
    /// run holds, as (module path, specifier); `None` past the work bound
    /// (every site may run). A site no body that can run holds never imports
    /// its target.
    pub fn live_dynamic_imports(&self) -> Option<&std::collections::BTreeSet<(String, String)>> {
        self.live.as_ref()
    }

    /// The set over `sorted`, a subset of the pass's modules (dynamic reach
    /// drops only modules dynamic-only sites load: the pass never walked
    /// them, so no active key names them).
    fn over(self, sorted: &[(&str, &Module)]) -> Option<model::Act> {
        let mut act = self.act?;
        let index: std::collections::HashMap<&str, u32> = sorted
            .iter()
            .enumerate()
            .map(|(i, (p, _))| (*p, i as u32))
            .collect();
        let modules = &self.modules;
        let to = |s: crate::pts::FxHashSet<(u32, u32, String)>| {
            s.into_iter()
                .filter_map(|(m, id, name)| match modules.get(m as usize) {
                    Some(path) => Some((*index.get(path.as_str())?, id, name)),
                    // A synthetic function's key names no module.
                    None => Some((m, id, name)),
                })
                .collect()
        };
        act.on = to(std::mem::take(&mut act.on));
        act.walked = to(std::mem::take(&mut act.walked));
        act.seen = to(std::mem::take(&mut act.seen));
        Some(act)
    }
}

/// The activation pass over `modules` ([`Activation`]): the module bodies
/// run; a body is walked once an active body's transfer solves to its
/// function or the function escapes, until none is left.
pub fn activation(modules: &[(String, &Module)], cfg: &FactsConfig) -> Activation {
    let mut sorted: Vec<(&str, &Module)> = modules.iter().map(|(p, m)| (p.as_str(), *m)).collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let (act, live) = match activation_pass(&sorted, cfg, std::time::Instant::now()) {
        Some((a, seen)) => (
            Some(a),
            Some(
                seen.into_iter()
                    .map(|(mi, spec)| (sorted[mi as usize].0.to_owned(), spec))
                    .collect(),
            ),
        ),
        None => (None, None),
    };
    Activation {
        act,
        modules: sorted.iter().map(|(p, _)| (*p).to_owned()).collect(),
        live,
    }
}

fn activation_pass<'a>(
    sorted: &[(&'a str, &'a Module)],
    cfg: &FactsConfig,
    t0: std::time::Instant,
) -> Option<(model::Act, crate::pts::FxHashSet<(u32, String)>)> {
    let sabotage = cfg
        .sabotage
        .as_deref()
        .and_then(|s| s.strip_prefix("activation:"))
        .map(str::to_string);
    let act = model::Act {
        sabotage,
        ..Default::default()
    };
    let (mut g, _, _) = generate(sorted, cfg, None, None, Some(act), Hold::Midway, t0);
    crate::diagnostics::keysnap_plan(&mut g);
    g.m.solve();
    while !g.m.aborted && g.walk_activated() {
        g.m.solve();
    }
    if cfg.profile {
        // One line per pass; the key digest compares two passes' sets.
        let digest = g.m.act.as_ref().map_or(0u64, |a| {
            a.on.iter().fold(0u64, |h, (m, id, name)| {
                let mut x = std::collections::hash_map::DefaultHasher::new();
                let path = sorted.get(*m as usize).map_or("", |s| s.0);
                std::hash::Hash::hash(&(path, id, name), &mut x);
                h ^ std::hash::Hasher::finish(&x)
            })
        });
        eprintln!(
            "perry: wpfacts activation pass: {} active of {} functions, keys {digest:016x}{} ({:.0} ms)",
            g.m.act.as_ref().map_or(0, |a| a.on.len()),
            g.m.funcs.len(),
            if g.m.aborted { ", past the bound" } else { "" },
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }
    if cfg.profile {
        eprintln!(
            "perry: wpfacts activation pass profile: {}",
            g.m.size_line()
        );
        g.m.prof_mark(crate::model::NO_NODE, "activation end");
        g.m.prof_report();
        let mut v: Vec<_> = g.m.label_work.iter().map(|(k, v)| (*k, *v)).collect();
        v.sort_by(|a, b| (b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)));
        for (k, (es, ec, pops)) in v.iter().take(25) {
            eprintln!(
                "perry: wpfacts activation pass labelwork {k} succ={es} cons={ec} pops={pops}"
            );
        }
    }
    if g.m.aborted {
        return None;
    }
    let seen = (0..g.m.funcs.len() as u32)
        .map(|f| g.m.act_key(f))
        .collect();
    let mut a = g.m.act.take()?;
    a.fixed = true;
    a.queue.clear();
    a.seen = seen;
    Some((a, std::mem::take(&mut g.dynimport_seen)))
}

fn compute_act<'a>(
    modules: &'a [(String, &'a Module)],
    cfg: &FactsConfig,
    published: Option<&'a units::Published>,
    view: Option<&'a decl_units::UnitView>,
    // DECISIONS 225: `None` per unit (every body runs); `Some(None)` runs
    // the activation pass; `Some(Some(a))` is the compile's pass, given.
    gate: Option<Option<Activation>>,
) -> ProgramFacts {
    let t0 = std::time::Instant::now();
    let mut sorted: Vec<(&str, &Module)> = modules.iter().map(|(p, m)| (p.as_str(), *m)).collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    // DECISIONS 225 (the whole program): the facts are over the bodies that
    // can run. Per unit every body runs (its published functions are called
    // by units it cannot see).
    let act = match gate {
        None => None,
        Some(Some(given)) => given.over(&sorted),
        // D2 (owner 11-24, PROF C1 A): the final solve decides the set.
        Some(None) => Some(model::Act {
            inert: true,
            sabotage: cfg
                .sabotage
                .as_deref()
                .and_then(|s| s.strip_prefix("activation:"))
                .map(str::to_string),
            ..Default::default()
        }),
    };
    let activation = act.as_ref().map(|a| a.on.len());
    let again = act.clone();
    let (mut g, mut gen_ms, mut t1) = generate(&sorted, cfg, published, view, act, Hold::Once, t0);
    crate::diagnostics::keysnap_plan(&mut g);
    let activation = activation.map(|n| format!("active={n} functions={}", g.m.funcs.len()));
    // The whole program: a hold met in generation or the solve restarts the
    // analysis with the realm held from the seed (`Model::restart_on_hold`).
    // Provenance runs (they trace the first hold) keep the one analysis.
    g.m.solve();
    if g.m.hold_restart {
        let why = g.m.hold_why.take();
        drop(g);
        let t2 = std::time::Instant::now();
        let (g2, _, t3) = generate(&sorted, cfg, published, view, again, Hold::Seeded(why), t2);
        g = g2;
        gen_ms += t3.duration_since(t2).as_secs_f64() * 1000.0;
        t1 = t3;
        crate::diagnostics::keysnap_plan(&mut g);
        g.m.solve();
    }
    let first_solve_ms = t1.elapsed().as_secs_f64() * 1000.0;
    // Past the bound the unit claims nothing (sound): its contracts are
    // neither checked nor published (113-CWL3).
    let mut checks = 0u64;
    while cfg.unit && !g.m.aborted && {
        checks += 1;
        // DECISIONS 171: a cell its solve came to consume is seeded first.
        let woke = g.wake_cells();
        // What the check added is solved before its results are read
        // again, whether or not the check counted it as a change.
        g.contract_check() | woke | g.m.unsolved()
    } {
        g.m.solve();
    }
    let checks_ms = t1.elapsed().as_secs_f64() * 1000.0 - first_solve_ms;
    // D2: the set at the fixpoint is final: the walked bodies outside it
    // were inert.
    let mut activation = activation;
    if g.m.act.as_ref().is_some_and(|a| a.inert && !a.fixed) {
        let a = g.m.act.as_ref().unwrap();
        let live: Vec<model::FnIdx> = a
            .walked_fns
            .iter()
            .copied()
            .filter(|&f| a.state.get(f as usize) != Some(&model::FN_INERT))
            .collect();
        let walked = live.into_iter().map(|f| g.m.act_key(f)).collect();
        let seen = (0..g.m.funcs.len() as u32).map(|f| g.m.act_key(f)).collect();
        // An inert body's sites get no verdict (generic code), as an
        // unwalked body's never had one.
        let inert = |m: &model::Model, f: model::FnIdx| m.fn_inert(f);
        let m = &g.m;
        g.read_sites.retain(|s| !inert(m, s.func));
        g.store_sites.retain(|s| !inert(m, s.func));
        let walks = std::mem::take(&mut g.inert_walks);
        g.unwalked.extend(
            walks
                .into_iter()
                .filter(|(f, _)| m.fn_inert(*f))
                .map(|(_, w)| w),
        );
        let a = g.m.act.as_mut().unwrap();
        a.walked = walked;
        a.seen = seen;
        a.fixed = true;
        let inert = a.state.iter().filter(|&&s| s == model::FN_INERT).count();
        activation = Some(format!(
            "active={} functions={} inert={inert}",
            a.on.len(),
            g.m.funcs.len()
        ));
        if cfg.profile {
            eprintln!(
                "perry: wpfacts activation (final solve): {} active of {} functions, {} walked, {inert} inert",
                a.on.len(),
                g.m.funcs.len(),
                a.walked.len()
            );
        }
    }
    let escaped_inactive = g.m.act.is_some().then(|| g.m.escaped_inactive());
    g.m.settle_keyed_uses();
    g.settle_parent_layouts();
    g.m.prof_mark(crate::model::NO_NODE, "after solve");
    if cfg.profile {
        g.m.prof_report();
        eprintln!(
            "perry: wpfacts post-solve: first solve {first_solve_ms:.0} ms, {checks} contract checks with their solves {checks_ms:.0} ms, settle {:.0} ms",
            t1.elapsed().as_secs_f64() * 1000.0 - first_solve_ms - checks_ms
        );
    }
    if cfg.profile || cfg.explain {
        // One count per instance key kind (`Summaries::inst_key`).
        let mut kinds = [0usize; 10];
        let mut fnkeyed = 0;
        let mut fnval = 0;
        for &(k, l, _, v) in g.m.sum.inst_key.keys() {
            kinds[k as usize] += 1;
            fnval += (v != summary::NO_VAL) as usize;
            fnkeyed += (k == 1 && l == summary::LAY_NONE) as usize;
        }
        eprintln!(
            "perry: wpfacts instances {} (site {} layout {} of them function-keyed {}, birth {}, entry {} group {} for-in {}), program calls {}, calls {}, value runs {}",
            g.m.sum.insts.len(),
            kinds[0],
            kinds[1],
            fnkeyed,
            kinds[5],
            kinds[2],
            kinds[3] + kinds[4],
            kinds[6],
            g.m.sum.n_prog,
            g.m.calls.len(),
            fnval
        );
    }
    if cfg.profile {
        let mut v: Vec<_> = g.m.label_work.iter().map(|(k, v)| (*k, *v)).collect();
        v.sort_by(|a, b| (b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)));
        for (k, (es, ec, pops)) in v.iter().take(25) {
            eprintln!("perry: wpfacts labelwork {k} succ={es} cons={ec} pops={pops}");
        }
        let m = &g.m;
        let mut gs: Vec<_> = m
            .acc_groups
            .iter()
            .map(|((h, k), gr)| {
                let ng = match k {
                    model::Key::Named(a) => m.accessors.get(&(*h, *a)).map_or(0, |x| x.0.len()),
                    model::Key::Any(_) => m.accessor_keys.get(h).map_or(0, |x| x.len()),
                };
                (
                    m.nodes[gr.ar as usize].pts.len(),
                    m.nodes[gr.gt as usize].pts.len(),
                    m.nodes[gr.ar as usize].succ.len(),
                    ng,
                    *h,
                    *k,
                )
            })
            .collect();
        gs.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        for (ar, gt, succ, ng, h, k) in gs.iter().take(12) {
            eprintln!(
                "perry: wpfacts accgroup {} key={} ar={ar} gt={gt} ar_succ={succ} accessors={ng}",
                m.tok_name(*h),
                m.key_name(*k)
            );
        }
    }
    g.m.dump_meta();
    // Whether the analysis runs beside code it does not see
    // (`Model::foreign_code`): every rule that keeps a whole-program premise
    // reads it.
    if std::env::var("PERRY_FACTS_REPORT").is_ok_and(|v| v == "1") {
        eprintln!(
            "perry: wpfacts: foreign code {} (unit {} unseen module {} interpreter linked {} runs {})",
            g.m.foreign_code() as u8,
            g.m.unit_mode as u8,
            g.m.unseen_module_runs as u8,
            g.m.interp_linked as u8,
            g.m.interp_runs as u8
        );
    }
    #[cfg(test)]
    tests::delivery_boundaries::record(&g.m);
    #[cfg(test)]
    tests::amp_flow::record(&g);
    #[cfg(test)]
    tests::computed_key_members::record(&g.m);
    #[cfg(test)]
    tests::box_computed_read::record(&g.m);
    #[cfg(test)]
    tests::described_holder_computed_read::record(&g.m);
    if g.m.explain && !g.m.bf_mints.is_empty() {
        let mut v: Vec<(&str, u32)> = g.m.bf_mints.iter().map(|(k, n)| (*k, *n)).collect();
        v.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        eprintln!("perry: wpfacts explain: builtin-fn minted: {v:?}");
    }
    let solve_ms = t1.elapsed().as_secs_f64() * 1000.0;
    // Decision 150: interpreted code is the ⊤ summary node, never a
    // program-wide kill. Every value it can receive (arguments and results
    // of ⊤ calls, globals (E20), the scope object of a direct eval) already
    // escapes to ⊤; a ⊤ call that may be the Function constructor only
    // connects already-⊤ values. Its writes to intrinsic prototypes are the
    // runtime's guard word (`ProgramFacts::interp_guard`).
    let mut interp_guard = None;
    if let Some(linked) = &cfg.dynamic_code {
        if g.m.interp_runs || g.m.dyn_eval.is_some() || g.m.top_calls > 0 {
            interp_guard = Some(format!(
                "E14 interpreter linked ({linked}) and reachable ({})",
                g.m.dyn_eval.clone().unwrap_or_else(|| format!(
                    "{} calls of unknown function values",
                    g.m.top_calls
                ))
            ));
        }
    }
    if cfg.dynamic_code_reach.globals && interp_guard.is_none() {
        let first = cfg
            .dynamic_code_reach
            .sites
            .iter()
            .next()
            .cloned()
            .unwrap_or_default();
        interp_guard = Some(format!("E14 dynamic_code_reach: {first}"));
    }
    let t2 = std::time::Instant::now();
    g.resolve_param_guards();
    // I32 at rest (PR A): a field's representation (`FieldFacts::rep`, a
    // Number field's I32 or an `Arr` field's `elem`) reads int demand, so
    // demand is solved before the derive. It reads only the solved graph.
    // −0 tolerance (PR D): settle every interval that may hold `-0` before
    // anything reads one (demand, the derive, the bindings).
    if g.m.aborted && std::env::var_os("PERRY_FACTS_ABORT_TOP").is_some() {
        g.m.abort_top();
    }
    let zero_sign_tolerated = if g.m.aborted {
        0
    } else {
        zero_sign::solve(&mut g.m)
    };
    let demand_first = demand::at_rest_demand();
    if demand_first && !g.m.aborted && g.int_ranges && demand::enabled() {
        demand::solve(&mut g.m);
    }
    let (mut templates, mut sites) = if g.m.aborted {
        (Vec::new(), Vec::new())
    } else {
        derive::derive(&mut g)
    };
    k3_census::write(&g);
    g.m.rfam_report();
    // Value records: a verdict per literal template, after
    // the templates and sites, before any representation reads them.
    g.m.record_array_join_sabotage = cfg.sabotage.as_deref() == Some("vrarrjoin");
    let record_arrays = records::derive(
        &g,
        &mut templates,
        &sites,
        interp_guard.as_deref(),
        cfg.sabotage.as_deref(),
    );
    g.m.records = templates
        .iter()
        .filter(|t| t.record.is_ok())
        .map(|t| t.id)
        .collect();
    g.m.record_arrays = record_arrays
        .iter()
        .filter(|a| a.verdict.is_ok() && g.m.records.contains(&a.elems_of))
        .map(|a| (a.tmpl, a.elems_of))
        .collect();
    records::mark_fields(&g.m, &mut templates);
    if g.m.value_records && std::env::var("PERRY_VALUE_RECORDS_TRACE").as_deref() == Ok("1") {
        for l in records::ledger_lines(&templates, &record_arrays) {
            eprintln!("perry: wpfacts: {}", l.trim_start_matches("# "));
        }
    }
    let mut elements = if g.m.aborted {
        Vec::new()
    } else {
        derive::element_facts(&g)
    };
    if !demand_first && !g.m.aborted && g.int_ranges && demand::enabled() {
        demand::solve(&mut g.m);
    }
    // I32 at rest (PR B): the at-rest templates, which `ArrRep::at_rest`
    // (`bindings::collect`) reads. After demand: an element rep reads it.
    if !g.m.aborted {
        let (rest, refused) = rest_elems::admit(&g, interp_guard.is_some());
        (g.m.rest_tmpls, g.m.rest_refused) = (rest, refused);
        rest_elems::mark_fields(&g.m, &mut templates);
    }
    let mut bindings = bindings::collect(&g);
    rest_elems::export(&g, &mut bindings);
    records::export(&g, &mut bindings);
    fill_births::export(&g.m, &mut bindings);
    push_fills::export(&g.m, &mut bindings);
    // I32 at rest (PR C): the I32 field lanes (`FieldFacts::rest`).
    let rest_lanes = if g.m.aborted {
        Vec::new()
    } else {
        rest_fields::admit(&g, &mut templates, interp_guard.is_some())
    };
    rest_fields::export_reads(&g, &templates, &mut bindings);
    let mut rep_probe = if cfg.diagnostics && !g.m.aborted {
        let mut p = bindings::rep_probe(&g);
        p.extend(rest_lanes);
        p.extend(literal_strings::probe(&g, &templates));
        p.extend(booleans::probe(&g, &templates));
        if g.m.zero_sign {
            p.push(format!("# zero-sign tolerated={zero_sign_tolerated}"));
        }
        p
    } else {
        Vec::new()
    };
    if !g.m.aborted && field_read_calls::enabled() {
        field_read_calls::collect(
            &g,
            &sites,
            &mut bindings,
            interp_guard.is_some(),
            cfg.sabotage.as_deref() == Some("fieldreadcall"),
            cfg.sabotage.as_deref() == Some("arrb1"),
        );
    }
    if canonical_numbers::enabled() {
        canonical_numbers::collect(
            &g,
            &mut bindings,
            cfg.sabotage.as_deref(),
            cfg.diagnostics.then_some(&mut rep_probe),
        );
    }
    if call_kinds::enabled() {
        call_kinds::collect(
            &g,
            &mut bindings,
            cfg.sabotage.as_deref(),
            cfg.diagnostics.then_some(&mut rep_probe),
        );
    }
    list_caps::collect(&g, &mut bindings, cfg.diagnostics.then_some(&mut rep_probe));
    if cfg.verify_facts {
        element_floors::collect(&g, &mut bindings);
    }
    if g.m.const_numbers {
        const_numbers::collect(&g, &mut bindings);
    }
    const_arrays::collect(&g, &mut bindings);
    held_lengths::collect(
        &g,
        &mut bindings,
        interp_guard.is_some(),
        cfg.sabotage.as_deref() == Some("heldlen"),
        cfg.diagnostics.then_some(&mut rep_probe),
    );
    named_lengths::collect(
        &g,
        &mut bindings,
        interp_guard.is_some(),
        cfg.sabotage.as_deref() == Some("namedlen"),
        cfg.diagnostics.then_some(&mut rep_probe),
    );
    gen_sources::collect(
        &g,
        &mut bindings,
        interp_guard.is_some(),
        cfg.sabotage.as_deref() == Some("gensrc"),
        cfg.diagnostics.then_some(&mut rep_probe),
    );
    quiet_sites::collect(
        &g,
        &sites,
        &mut bindings,
        interp_guard.is_some(),
        cfg.sabotage.as_deref(),
        cfg.diagnostics.then_some(&mut rep_probe),
    );
    method_sites::collect(
        &g,
        &mut bindings,
        interp_guard.is_some(),
        cfg.sabotage.as_deref(),
        cfg.diagnostics.then_some(&mut rep_probe),
    );
    element_keys::collect(&g, &mut bindings);
    element_keys::collect_signs(&g, &mut bindings);
    element_keys::collect_wide(&g, &mut bindings);
    gen::in_bounds::collect(&g, &mut bindings);
    gen::in_bounds::collect_reads(&g, &mut bindings);
    if !g.m.aborted && signatures::enabled() {
        signatures::collect(
            &g,
            &sites,
            &mut bindings,
            interp_guard.is_some(),
            cfg.sabotage.as_deref(),
            cfg.diagnostics.then_some(&mut rep_probe),
        );
    }
    if g.m.aborted {
        // Over budget: claim nothing (sound), keep the shapes for the dump.
        templates.clear();
        sites.clear();
        elements.clear();
        bindings.clear();
    }
    if let Some(spec) = &cfg.sabotage {
        dump::apply_sabotage(&mut templates, spec);
    }
    let derive_ms = t2.elapsed().as_secs_f64() * 1000.0;
    if cfg.profile {
        eprintln!(
            "perry: wpfacts post-solve: derive {derive_ms:.0} ms, {} markers' members settled",
            g.m.members_settled.borrow().len()
        );
    }
    let stats = Stats {
        modules: sorted.len(),
        templates: g.m.tmpls.len(),
        fact_units: templates.len(),
        nodes: g.m.nodes.len(),
        constraints: g.m.stats_cons,
        tokens: g.m.toks.len(),
        functions: g.m.funcs.len(),
        gen_ms,
        solve_ms,
        derive_ms,
        pops: g.m.pops,
        instances: g.m.sum.insts.len() as u64,
        shared_instances: g
            .m
            .sum
            .inst_key
            .keys()
            .filter(|k| k.0 == 1 && k.1 == summary::LAY_NONE && k.3 == summary::NO_VAL)
            .count() as u64,
        composed: g.m.sum.ncomposed as u64,
        solver_calls: g.m.solver_made,
        contexts: g.m.sum.ctx_nodes.len() as u64,
        work: g.m.work.get(),
        aborted: g.m.aborted,
        widened: g.m.widened,
        widened_stop: g.m.widened_stop,
        checks,
        pts_entries: g.m.pts_entries,
        big_nodes: big_nodes(&g.m),
        builtin_fields: builtin_fields(&g.m),
        top_calls: g.m.top_calls,
        field_truthy: field_truthy_filters(&g.m),
        field_truthy_rows: field_truthy_rows(&g.m),
        solve_counters: solve_counters(&g.m),
        profile: {
            let mut v: Vec<(String, u64, f64)> =
                g.m.profile
                    .iter()
                    .flatten()
                    .map(|(k, (n, d))| (k.to_string(), *n, d.as_secs_f64() * 1000.0))
                    .collect();
            v.sort_by(|a, b| b.2.total_cmp(&a.2));
            v
        },
    };
    let tok_names = if cfg.relations {
        (0..g.m.toks.len() as u32)
            .map(|t| g.m.tok_name(t))
            .collect()
    } else {
        Vec::new()
    };
    let (array_iteration_written, array_iterator_close_written) = if stats.aborted {
        (
            Some("solve aborted".to_string()),
            Some("solve aborted".to_string()),
        )
    } else {
        g.m.array_iteration_writes()
    };
    let native_extern_sites: Vec<(String, Option<u32>)> =
        g.m.native_extern_fired
            .iter()
            .map(|&(mi, off)| {
                (
                    sorted
                        .get(mi as usize)
                        .map_or(String::new(), |s| s.0.to_owned()),
                    off,
                )
            })
            .collect();
    let fnproto_bind_written = if stats.aborted {
        Some("solve aborted".to_string())
    } else {
        g.m.fnproto_write.map(|r| g.m.reasons[r as usize].clone())
    };
    // Per unit, a class's own unit says whether it is relinked: another
    // unit's class (a stand-in) may be relinked where this unit does not
    // see it, and the driver joins every unit's claims.
    let super_unrelinked = if stats.aborted {
        Default::default()
    } else {
        g.m.toks
            .iter()
            .enumerate()
            .filter_map(|(t, kind)| {
                let model::TokKind::ClassObj(tm) = kind else {
                    return None;
                };
                if g.m.foreign.contains_key(tm) {
                    return None;
                }
                let model::TmplKind::Class { class_id, .. } = g.m.tmpls[*tm as usize].kind else {
                    return None;
                };
                if g.m.parent_unsettled.contains(tm) {
                    return None;
                }
                let parent =
                    g.m.tmpls[*tm as usize]
                        .parent
                        .and_then(|p| match g.m.tmpls[p as usize].kind {
                            model::TmplKind::Class { class_id, .. } => Some(class_id),
                            _ => None,
                        });
                let origin = g.m.constructor_origin(*tm);
                let owner =
                    g.m.tok_index
                        .get(&model::TokKind::ClassObj(origin))
                        .copied()
                        .unwrap_or(t as u32);
                (!g.m.is_escaped(owner) && !g.m.super_relink.contains(&owner))
                    .then_some((class_id, parent))
            })
            .collect()
    };
    let diagnostics = if cfg.diagnostics {
        vec![(view.map_or("<whole>", |v| v.key.as_str()).to_owned(), {
            let rows = diagnostics::collect(&g);
            rows
        })]
    } else {
        Vec::new()
    };
    let diagnostic_sites = if cfg.diagnostics {
        diagnostics::recorded(&g)
    } else {
        Vec::new()
    };
    // Every body the analysis walked, and every body holding a site (a
    // context generation enters without a walk of its own).
    let bodies =
        g.m.act
            .as_ref()
            .map(|a| {
                let holding = g
                    .read_sites
                    .iter()
                    .filter(|s| !g.m.fn_inert(s.func))
                    .map(|s| g.m.act_key(s.func));
                // `on` is the activate predicate itself; a body it admits is
                // reached even when generation walked it under no pending entry.
                a.walked
                    .iter()
                    .cloned()
                    .chain(a.on.iter().cloned())
                    .chain(holding)
                    .filter_map(|(mi, id, name)| {
                        Some((sorted.get(mi as usize)?.0.to_owned(), id, name))
                    })
                    .collect()
            })
            .unwrap_or_default();
    let seen: std::collections::BTreeSet<BodyIdentity> =
        g.m.act
            .as_ref()
            .map(|a| {
                a.seen
                    .iter()
                    .filter_map(|(mi, id, name)| {
                        Some((sorted.get(*mi as usize)?.0.to_owned(), *id, name.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default();
    let body_reach = BodyReach {
        bodies,
        seen,
        complete: g.m.act.is_some() && !stats.aborted,
    };
    let fnproto_borrow_written = if stats.aborted {
        Some("solve aborted".to_string())
    } else {
        g.m.fnproto_borrow_write
            .map(|r| g.m.reasons[r as usize].clone())
    };
    let verification_bodies = if (cfg.verify_facts || cfg.dce_bodies) && body_reach.complete {
        Some(
            body_reach
                .seen
                .iter()
                .map(|k| (k.clone(), !body_reach.inactive(k)))
                .collect(),
        )
    } else {
        None
    };
    let family_reach =
        (!cfg.unit && family_reach::enabled()).then(|| family_reach::derive(&g, cfg));
    let native_dispatch = family_reach
        .is_some()
        .then(|| family_reach::derive_dispatch(&g, cfg));
    let member_ops = match native_dispatch {
        Some(_) => derive::member_op_facts(&g, &bindings, interp_guard.is_some()),
        None => Vec::new(),
    };
    let mut pf = ProgramFacts {
        verification_bodies,
        family_reach,
        native_dispatch,
        member_ops,
        diag_stream: None,
        diagnostics,
        diagnostic_sites,
        activation,
        escaped_inactive,
        body_reach,
        super_unrelinked,
        fnproto_bind_written,
        native_extern_sites,
        array_iteration_written,
        array_iterator_close_written,
        fnproto_borrow_written,
        module_paths: sorted.iter().map(|(p, _)| p.to_string()).collect(),
        templates,
        record_arrays,
        sites,
        elements,
        stats,
        interp_guard,
        dynamic_code_reach: cfg.dynamic_code_reach.clone(),
        sabotage: cfg.sabotage.clone(),
        relations: {
            crate::diagnostics::keyread(&g);
            g.m.prov_finish();
            g.m.relations.take()
        },
        rep_probe,
        zero_sign_tolerated,
        tok_names,
        bindings,
        indexed_metadata_unwritten: !g.m.aborted && g.m.native_writes.is_empty(),
        module_digests: Vec::new(),
    };
    derive::dispatch::certify(&mut pf, &g, cfg);
    pf.module_digests = dump::module_digests(&pf);
    pf
}

impl ProgramFacts {
    /// DESIGN-native-dispatch PR 1: every dispatcher obligation, each owned
    /// by its site: a named member call ([`SiteFacts`]) or a HIR-specialized
    /// member op ([`MemberOpFacts`]). The list is every route into the
    /// dispatcher only when [`ProgramFacts::native_dispatch`] is complete.
    pub fn dispatch_obligations(&self) -> impl Iterator<Item = DispatchObligation<'_>> {
        self.sites
            .iter()
            .filter_map(SiteFacts::dispatch_obligation)
            .chain(
                self.member_ops
                    .iter()
                    .map(MemberOpFacts::dispatch_obligation),
            )
    }
}

/// `PERRY_FACTS_QUIET_SITES=0` turns the quiet-site facts off
/// (`quiet_sites`).
pub fn quiet_sites_enabled() -> bool {
    quiet_sites::enabled()
}

/// `PERRY_FACTS_TYPED_ARRAY_SITES=0` turns the typed-array receiver fact
/// off (`method_sites`).
pub fn typed_array_sites_enabled() -> bool {
    method_sites::typed_array_enabled()
}

/// `PERRY_FACTS_ARRAY_METHOD_SITES=0` turns the Array-member fact off
/// (`method_sites`).
pub fn array_method_sites_enabled() -> bool {
    method_sites::array_member_enabled()
}

/// `PERRY_PROGRAM_FACTS=0` turns the pass off (every fact off; digest 0).
pub fn enabled_from_env() -> bool {
    !matches!(
        std::env::var("PERRY_PROGRAM_FACTS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// A contract function's name and, when it is closed, its closed key.
pub(crate) fn fn_contract_name(g: &gen::Gen, f: model::FnIdx) -> (String, Option<String>) {
    let fi = &g.m.funcs[f as usize];
    let fid = fi.func_id;
    let closed = g.view.and_then(|v| {
        v.closed
            .iter()
            .find(|&(_, &id)| id == fid)
            .map(|(nm, _)| nm.clone())
    });
    let name = match &closed {
        Some(k) if k.contains('#') && !fi.name.ends_with(&format!("#{fid}")) => {
            format!("{}#{fid}", fi.name)
        }
        _ => fi.name.clone(),
    };
    (name, closed)
}
