// #2014 follow-up: a plain-object validator passed to `assert.throws` must
// treat a RegExp value on a key (e.g. `message: /bad/`) as a `regex.test`
// against the thrown error's prop, NOT as a deep-equal compare. Without
// this, `{ code: "X", message: /bad/ }` rejected every error because the
// thrown `message` string was never `=== /bad/`.
import assert from "node:assert";

function check(label: string, fn: () => void): void {
  try { fn(); console.log(label + ": pass"); }
  catch (err: any) { console.log(label + ":", err?.name, err?.code || err?.operator || "no-code"); }
}

// `message: RegExp` matches when the regex tests against the thrown message.
check(
  "object {message:/bad/} matches",
  () =>
    assert.throws(
      () => { throw new Error("bad input"); },
      { message: /bad/ },
    ),
);

// And rejects when it doesn't.
check(
  "object {message:/good/} rejects",
  () =>
    assert.throws(
      () => { throw new Error("bad input"); },
      { message: /good/ },
    ),
);

// Combined: every key must hold — code is === and message is a RegExp.
check(
  "object {code, message:/bad/} matches",
  () => {
    const e: any = new Error("bad input"); e.code = "ERR_X";
    assert.throws(() => { throw e; }, { code: "ERR_X", message: /bad/ });
  },
);

// And one wrong key still rejects.
check(
  "object {code:wrong, message:/bad/} rejects",
  () => {
    const e: any = new Error("bad input"); e.code = "ERR_X";
    assert.throws(() => { throw e; }, { code: "ERR_Y", message: /bad/ });
  },
);

// name RegExp also tests the thrown error's name (a class string).
check(
  "object {name:/Type/} matches TypeError",
  () => assert.throws(() => { throw new TypeError("x"); }, { name: /Type/ }),
);
