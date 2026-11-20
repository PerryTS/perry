// Regression test for #679 follow-up — `Box(rows)` where `rows` is a
// runtime value (e.g. `msgs.map(m => Text(m))`) previously produced
// an empty Box because the codegen recogniser only walked `Expr::Array`
// literals. Pre-fix, the user's chat demo rendered the header + divider
// + prompt but never the messages.
//
// This test seeds a useState array, returns Box({}, msgs.map(...)),
// and asserts the rendered ANSI contains the row text. Uses a
// ref-counter exit pattern so multiple frames paint before quitting.

import {
    Box,
    Text,
    useState,
    useRef,
    useEffect,
    useInput,
    exit,
    run,
} from "perry/tui";

let last_len = -1;

run(() => {
    const [msgs, setMsgs] = useState([] as string[]);
    const ctr = useRef(0);

    useEffect(() => {
        setMsgs(["ROW_ALPHA", "ROW_BETA", "ROW_GAMMA"]);
    }, []);

    last_len = msgs.length;

    const c = ctr.get();
    if (c >= 4) exit();
    else ctr.set(c + 1);

    useInput((_s: string) => {});

    // 2-arg shape: opts + runtime-array children.
    return Box({ flexDirection: "column" }, msgs.map((m: string) => Text(m)));
});

console.log("LAST_LEN=" + last_len);
