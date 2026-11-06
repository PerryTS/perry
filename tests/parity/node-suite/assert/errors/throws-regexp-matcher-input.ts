// A RegExp matcher is tested against `String(thrown)` and nothing else, and it
// is a terminal matcher category: a non-match is an AssertionError, never a
// fallthrough to the instanceof / constructor / validation-function checks.
// The pre-existing `strict-throws-validation.ts` only exercised a pattern that
// matches the message *and* the stringified error, so it could not tell the
// two inputs apart.
import assert from "node:assert";

function show(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": pass");
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code ?? err?.operator ?? "no-code");
  }
}

// `String(new Error("nope"))` is "Error: nope", so an anchored pattern that
// only matches the bare message must NOT match.
show("anchored message-only pattern", () =>
  assert.throws(() => { throw new Error("nope"); }, /^nope/));

// The same pattern anchored against the stringified error does match.
show("anchored stringified pattern", () =>
  assert.throws(() => { throw new Error("nope"); }, /^Error: nope$/));

// A plain object is stringified to "[object Object]"; its `message` property
// is not a second chance for the pattern.
show("plain object with message prop", () =>
  assert.throws(() => { throw { message: "nope" }; }, /^nope/));

// A thrown primitive stringifies to itself.
show("thrown string", () =>
  assert.throws(() => { throw "nope"; }, /^nope$/));

// doesNotThrow with a non-matching RegExp rethrows the original error rather
// than reporting an AssertionError (and must not raise a TypeError from a
// fallthrough into `instanceof`).
show("doesNotThrow non-matching rethrows", () =>
  assert.doesNotThrow(() => { throw new Error("nope"); }, /will-not-match/));

show("doesNotThrow matching reports", () =>
  assert.doesNotThrow(() => { throw new Error("nope"); }, /nope/));

// A RegExp value on a validator *key* is a different matcher: it is tested
// against that property, so an anchored message pattern does match there.
show("validator key regexp", () =>
  assert.throws(() => { throw new TypeError("bad value"); }, { message: /^bad/ }));

// Async paths route through the same matcher.
await assert.rejects(async () => { throw new Error("nope"); }, /^nope/).then(
  () => console.log("rejects anchored: pass"),
  (err: any) => console.log("rejects anchored:", err?.name, err?.code ?? err?.operator));

await assert.doesNotReject(async () => { throw new Error("nope"); }, /will-not-match/).then(
  () => console.log("doesNotReject non-matching: pass"),
  (err: any) => console.log("doesNotReject non-matching:", err?.name, err?.code ?? "no-code"));
