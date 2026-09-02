**A global regex scan no longer drops the empty match that sits where the
previous match ended** — `"a".match(/a*/g)` is `["a",""]`, `"a".replace(/a*/g,
"<>")` is `"<><>"`, and the same for `matchAll` and every `replace` form.

```js
"a".match(/a*/g)         // was ["a"]        now ["a",""]
"aXa".match(/a*/g)       // was ["a","a"]    now ["a","","a",""]
"ab".match(/b*/g)        // was ["","b"]     now ["","b",""]
"a".replace(/a*/g, "<>") // was "<>"         now "<><>"
```

ECMAScript's `RegExp.prototype [ @@match ]` loop keeps a zero-width match at
the previous match's end and *then* advances one code unit
(`AdvanceStringIndex`). Rust's iterators do the opposite: both
`regex_automata`'s `Searcher::try_advance` and `fancy_regex`'s
`Matches::next_with` — the latter documented as "adapted from the `regex`
crate … ignores empty matches immediately after a match" — discard it and
re-search one character to the right. Every global operation was built on
those iterators, so every one inherited the rule.

**The reported symptom understated it.** The rule fires wherever an empty
match lands on a previous match's end, not only at the end of the subject, so
interior matches were lost too: `"aXa".match(/a*/g)` was missing *two* of
Node's four elements, and `"a1b22".match(/\d*/g)` three of five.

One `global_scan` module now holds the ECMAScript loop, and every global site
goes through it: `String#match`, `matchAll`, `replace`/`replaceAll` with a
string replacement, with a `$<name>` replacement, and with a callback — on
both the linear `regex` lane and the `fancy_regex` lookaround/backreference
lane. `regress`, the third engine, already stepped one position past a
zero-width match, which is the ECMAScript rule; its iterators are used
unchanged, and a test pins that lane as the control. `Regex::replace_all` is
gone from the string-replacement path for the same reason — it runs the
crate's iterator internally.

The scan takes a starting byte offset rather than a slice, which also gives
`matchAll` the #9429 treatment: it used to search
`&subject[lastIndex..]`, so a `matchAll` on a regex with a non-zero
`lastIndex` evaluated `^`, `\b` and lookbehind against the wrong left edge.

**`test_parity_regex_replace_fn_lookahead` diverged from Node because of
this**, exactly as #9430 recorded — and the runner could not see it, because
that test is scored against a stored `expected/…txt` holding `OK` rather than
against Node. Its `/[a-z]+|(?=\.)/g` assertion asked for `["ab","cd"]`, which
is the Rust iterator's answer; Node has always produced `["ab","","cd"]` and
thrown. The assertion now reads Node's answer, so both runtimes print `OK`.

**Found while fixing, NOT fixed here:** `split` by a pattern only fancy-regex
can compile does not run `RegExp.prototype [ @@split ]` at all — the fallback
walks `find_iter` and slices between matches. It therefore emits a trailing
`""` the spec's `q < size` bound never reaches (`"a,b,".split(/(?<=,)/)` →
`["a,","b,",""]` vs Node's `["a,","b,"]`) and splices no captured groups
(`"aXbXc".split(/((?<=a)X)/)` → `["a","bXc"]` vs Node's `["a","X","bXc"]`).
That is a lane gap rather than a scan gap — the `regex` lane runs the spec
algorithm in `spec_regex_split` and is correct — so it is excluded from this
fixture with a comment, and the runtime test `fancy_lookbehind_split`
currently pins the wrong answer.
