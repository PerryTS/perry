`assert.throws(fn, /re/)` and friends now test the RegExp matcher the way Node
does: against `String(thrown)` only, with a RegExp treated as a terminal
matcher category.

`expected_error_matches` used to give a non-matching pattern a second chance
against `thrown.message`, then fall through to the `instanceof` /
constructor-name / validation-function checks when that also failed. Two bugs
came out of one block:

- A pattern that matched the bare message but not the stringified error was
  wrongly accepted. `assert.throws(() => { throw new Error("nope") }, /^nope/)`
  passed, where Node reports an `AssertionError` because `String(err)` is
  `"Error: nope"`. Same for a thrown non-error carrying a `message` property.
- A pattern that matched nothing reached `js_instanceof_dynamic(thrown, regexp)`
  and surfaced as a `TypeError` instead of an `AssertionError`, so
  `assert.throws`/`rejects` reported the wrong error class and
  `doesNotThrow`/`doesNotReject` failed to rethrow the original error.

A RegExp *value on a validator key* (`{ message: /bad/ }`) is a different
matcher and still tests against that property.

The pre-existing `assert/errors/strict-throws-validation.ts` could not catch
either bug: its pattern matched both the message and the stringified error, and
it only printed `name`/`code`. `assert/errors/throws-regexp-matcher-input.ts`
covers the two inputs separately across `throws`, `doesNotThrow`, `rejects`,
and `doesNotReject`.

Known remaining gap: the generated `AssertionError` message is Perry's generic
"The thrown error did not match the expected matcher" rather than Node's
"The input did not match the regular expression /x/. Input:\n\n'Error: nope'\n".
That generic fallback is shared by every matcher category, so it is left for a
separate change.
