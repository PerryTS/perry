**`exec`/`test` at a non-zero `lastIndex` now evaluate the pattern against the
whole subject instead of `subject.slice(lastIndex)`** — `^`, `$`, `\b` and both
lookaround directions get their real context back. No flag beyond `g`/`y` was
needed to see this:

```js
const r = /^b/g;      r.lastIndex = 1; r.exec("ab")   // was "b",  now null
const l = /(?<=a)b/g; l.lastIndex = 1; l.exec("ab")   // was null, now "b"
```

The engine call sliced the subject at the start offset and then re-based every
reported range by the same amount. Offsets survived that round trip; assertions
did not. A slice invents context at its left edge — `^` and `\b` hold at
offset 0 of the slice, where the subject says they must not — and destroys it —
`(?<=a)` cannot see the character it needs, and `(?<!a)` therefore holds
everywhere. Under `/m` it was severe: a line-scanning `while ((m = re.exec(s)))`
loop saw `^` hold at *every* index, so it walked one character at a time and
never terminated on its own.

All three engines already expose a positional entry point documented to keep
the surrounding context — `regex::Regex::captures_at`,
`fancy_regex::Regex::captures_from_pos` and `regress::Regex::find_from` — and
each returns absolute offsets, so the re-basing arithmetic is gone rather than
adjusted. `OwnedExecMatch`'s three constructors no longer take a
`search_start_byte` at all: with the parameter removed, handing an engine a
slice again would not compile. The sticky check moves with it, from
`start() == 0` to `start() == lastIndex`.

**Found while fixing, same function:** `lastIndex > length` was not "no match"
(RegExpBuiltinExec step 12.a) but a search clamped to the end of the subject —
`/a*/g` with `lastIndex = 5` on `"ab"` returned an empty match at index 2 where
Node returns `null`. The bound could not be expressed where it was being
checked: it is a UTF-16 code-unit comparison, and `utf16_index_to_byte`
saturates at the payload length, so the byte-offset guard it replaced could
never fire. That also matters for astral subjects, where the code-unit length
and the scalar count differ.

Pinned by six runtime tests — one per engine lane, plus the past-the-end bound
and the `test` routing — and by a fixture byte-compared against Node covering
`^`, `$`, `\b`, `\B`, lookbehind, negative lookbehind and lookahead at
`lastIndex` 0 / mid-subject / end / past-end, sticky and global, and seven
hand-driven `exec` sweeps that have to terminate.

Two of those sweeps need #9408 (landed in #9427) as well as this fix, and are
the reason to read the pair together: `while ((m = /^/gm.exec("one\r\ntwo")))`
walks `[0, 4, 5]` — Node's answer — only with both. With #9408 alone the loop
never terminates, because `^` holds at the slice's left edge at every index;
with this fix alone it stops early at `[0, 5]`, because `(?m)` still sees LF
only.
