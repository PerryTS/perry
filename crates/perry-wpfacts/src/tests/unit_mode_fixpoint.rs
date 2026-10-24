/// ee-first (express, D16): `callback` reads `arguments` and is passed to
/// closed `listener` as `done`; the `onevent` it returns escapes through
/// `ee.on` and calls `done`.
const EE_FIRST: &str = r#"
    export default function first(stuff: any, done: any) {
        var cleanups: any[] = [];
        for (var i = 0; i < stuff.length; i++) {
            var arr = stuff[i];
            var ee = arr[0];
            for (var j = 1; j < arr.length; j++) {
                var event = arr[j];
                var fn = listener(event, callback);
                ee.on(event, fn);
                cleanups.push({ ee: ee, event: event, fn: fn });
            }
        }
        function callback() {
            cleanup();
            done.apply(null, arguments);
        }
        function cleanup() {
            var x;
            for (var i = 0; i < cleanups.length; i++) {
                x = cleanups[i];
                x.ee.removeListener(x.event, x.fn);
            }
        }
        function thunk(fn: any) {
            done = fn;
        }
        thunk.cancel = cleanup;
        return thunk;
    }
    function listener(event: any, done: any) {
        return function onevent(this: any, arg1: any) {
            var args = new Array(arguments.length);
            var ee = this;
            var err = event === "error" ? arg1 : null;
            for (var i = 0; i < args.length; i++) {
                args[i] = arguments[i];
            }
            done(err, ee, event, args);
        };
    }
"#;

/// Runs `f` on its own thread; `None` when it has not returned in `secs`.
fn bounded<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(secs)).ok()
}

#[test]
fn an_argument_contract_ascends_so_the_units_reach_a_fixpoint() {
    // D16 (express, ee-first): `callback` reads `arguments` (its argument
    // contract is ⊤) and is passed to closed `listener` as `done`. With ⊤
    // from `first`, `listener.done` is unknown, so no call reaches
    // `callback` and its contract drops to none; then `callback` is closed,
    // its function contract is published, `listener.done` holds it, the
    // escaped `onevent` calls it: ⊤ again. Rebuilt from the latest solves
    // only, the contract cycled with period 3 and the driver never ended.
    // Joined with last round's (it only grows), the run ends: rounds 1-3
    // pass the contracts on, round 4 changes nothing. Bounded by the test,
    // so a driver that cycles fails here instead of hanging the suite.
    let m = r#"import first from "./ee";
        const t = first([[process, "exit"]], function (err: any) { console.log(err); });
        t.cancel();"#;
    let pf = bounded(60, move || units_of(&[("ee", EE_FIRST), ("m", m)]))
        .expect("D16: the per-unit fixpoint did not end in 60 s");
    assert_eq!(pf.rounds, 4, "rounds {:?}", pf.round_work);
}
