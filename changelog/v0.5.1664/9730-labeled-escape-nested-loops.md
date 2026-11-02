**A labeled `break`/`continue` that targets an outer loop from inside a nested
loop now works in generators, async generators and async functions** (#9199).
It previously threw `TypeError: Cannot read properties of undefined (reading
'done')` for `break`, and silently produced nothing for `continue`.

```ts
async function* g() {
  O: for (const x of [1, 2]) { I: for (const y of [0, 1]) { yield "b" + x + y; break O; } }
}
// node: b10        before: TypeError … reading 'done'
```

Generator linearization gives each loop a single `break` sentinel and a single
`continue` sentinel, so a completion can only name the loop it sits in.
`rewrite_labeled_bc_in_stmts` therefore converted `break label` / `continue
label` to plain completions **only at the labeled loop's own body level** and
stopped at nested loops — correctly, since a plain completion inside a nested
loop would bind to that loop. What was missing is what happens to the escape
that is left: it survived verbatim into a state body, where the dispatch
lowering has no sentinel for it and dropped it. The limitation was noted in the
code ("the single-sentinel scheme can't yet distinguish targets").

Rather than teach the state machine to name a distant target, the escape is now
unwound one loop at a time through a carrier local, so every completion the
linearizer sees is plain and binds to the loop it is in:

```
__esc = 0;
inner: while (…) { … __esc = 1; break; … }   // was `break label`
if (__esc == 1) break;                        // in the labeled loop
if (__esc == 2) continue;
```

Deeper nesting reuses the same carrier and propagates outward with a bare
`if (__esc != 0) break;` after each intermediate loop. A `switch` that carries
an escape is desugared to `if`s first, since a plain `break` inside a switch
would bind to the switch.

The hole was wider than the issue's own repro, which #9189 had already closed:
it reached sync generators and async functions as well as async generators,
and the `switch` in the report was incidental — a bare `break outer` in a
nested loop failed on its own, while the switch-wrapped form worked because
#9186's routing already handled it.

`test-files/test_gap_9199_labeled_escape_nested_loops.ts` pins 13 shapes:
`break`/`continue` of an outer label from a nested loop in all three function
kinds, three-deep nesting, a `while` outer, an `await` before the escape, a
conditional escape, `try`/`finally` around it (finalizers still run in order),
a reused label name on a sibling loop, and the switch-wrapped form that already
worked. Unpatched the fixture throws on its first row and then hangs; patched
it is byte-identical to node 26.5.1.
