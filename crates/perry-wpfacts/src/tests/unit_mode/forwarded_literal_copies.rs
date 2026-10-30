//! effect's `cli/internal/parser.ts` (effect/cli `Command`), reduced: the
//! parse state's `mode` holds `{ _tag }` literals, born in
//! `createParseState` and replaced in `processValue`, and the state goes
//! through the units of the scan. Several units forward copies of one
//! literal shape: replaced, not joined, the last forwarder of a round won,
//! a reader solved before it saw the entry change at the end of every
//! round, and the rounds never ended (#13579).

use super::*;

const P: &str = r#"
const buildSubcommandIndex = () => {
};
const createParseState = () => ({
  mode: { _tag: "AwaitingFirstValue" }
});
const resolveFirstValue = (context) => {
  const subIndex = buildSubcommandIndex();
};
const processValue = (value, cursor, context, state) => {
  if (state.mode._tag === "AwaitingFirstValue") {
    const result = resolveFirstValue(state);
    state.mode = { _tag: "CollectingArguments" };
  }
};
const scanCommandLevel = () => {
  const state = createParseState();
  for (let token = cursor.take(); token; token = cursor.take()) {
    if (token._tag === "Value") {
      const subResult = processValue(token.value, cursor, context, state);
    }
  }
};
"#;

#[test]
fn forwarded_copies_of_one_literal_shape_reach_a_fixpoint() {
    let mods: &'static [(&'static str, &'static str)] = &[("p", P)];
    // Before: no fixpoint (the solve runs forever), so the bound is a wait.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(units_of(mods));
    });
    let pf = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("per-unit facts reach a fixpoint");
    assert!(pf.rounds <= 5, "rounds {}", pf.rounds);
}
