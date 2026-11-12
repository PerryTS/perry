//! LIFE G1: the lifetime claims of one body, as the census walk answers them.
//!
//! Under `PERRY_VERIFY_FACTS=1` the census (`life_census`) runs without its
//! TSV and hands each body's lowering the births it says die at a known
//! point: a `frame` local (dead when its binding is rebound and at each
//! return) and an argument birth whose callee fate is Dropped (dead right
//! after the call). Codegen turns each into a death call
//! (`verify_facts::life`); the runtime checks that no pointer to the object
//! is reached after it. The claims are the census verdicts themselves, never
//! a second derivation. Release builds compute none.
//!
//! Negative control: `PERRY_FACTS_SABOTAGE=fate:<function>#<i>` makes the
//! census read argument `i` of every direct call to `<function>` as Proven
//! Dropped, `fatebet:<function>#<i>` as Dropped on a bet.
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use perry_transform::effects::{Fate, ParamFate, Proof};

/// The birth a claim is about, as the claim record names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClaimBirth {
    /// `new`, `object`, `array` or `closure`.
    pub kind: &'static str,
    pub class: String,
    /// A `new`'s byte offset, else `#<ordinal>` in the census walk.
    pub offset: String,
    /// `proven` or `bet:<site>`.
    pub proof: String,
    /// The callee and parameter of a dropped argument (`-` for a frame local).
    pub fate: String,
}

/// The claims of one body.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LifeClaims {
    /// `frame` locals by local id.
    pub frame: BTreeMap<u32, ClaimBirth>,
    /// Dropped argument births by (call byte_offset, call byte_end, index).
    pub dropped_args: HashMap<(u32, u32, usize), ClaimBirth>,
}

impl LifeClaims {
    pub(crate) fn is_empty(&self) -> bool {
        self.frame.is_empty() && self.dropped_args.is_empty()
    }
}

/// The census runs for its claims in this compilation.
pub(crate) fn wanted() -> bool {
    crate::verify_facts::life_claims_on()
}

/// The `fate:`/`fatebet:` negative control: (function name, index, bet).
fn sabotage() -> Option<&'static (String, usize, bool)> {
    static S: OnceLock<Option<(String, usize, bool)>> = OnceLock::new();
    S.get_or_init(|| {
        let s = std::env::var("PERRY_FACTS_SABOTAGE").ok()?;
        let (bet, rest) = if let Some(r) = s.strip_prefix("fatebet:") {
            (true, r)
        } else {
            (false, s.strip_prefix("fate:")?)
        };
        let (name, i) = rest.rsplit_once('#')?;
        Some((name.to_string(), i.parse().ok()?, bet))
    })
    .as_ref()
}

/// The fate the census reads for argument `i` of a direct call to `callee`
/// (named `name`): the computed one, or the planted one under the control.
pub(crate) fn planted(name: Option<&str>, i: usize, computed: ParamFate) -> ParamFate {
    match (sabotage(), name) {
        (Some((n, at, bet)), Some(name)) if n == name && *at == i => ParamFate {
            fate: Fate::Dropped,
            proof: if *bet { Proof::Bet(0) } else { Proof::Proven },
            why: None,
        },
        _ => computed,
    }
}

pub(crate) fn proof_word(p: Proof) -> String {
    match p {
        Proof::Proven => "proven".to_string(),
        Proof::Bet(site) => format!("bet:{site}"),
    }
}
