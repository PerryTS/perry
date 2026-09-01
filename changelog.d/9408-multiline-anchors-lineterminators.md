### Fixed

- **`^` and `$` under the `m` flag now hold at every LineTerminator, not just
  LF (#9408).** ECMAScript §22.2.2.6 defines the multiline anchors over the
  same four characters a non-dotAll `.` excludes — `\n`, `\r`, U+2028 and
  U+2029 — but the translation leaned on Rust's `(?m)`, which recognizes LF
  alone. `"one\rtwo".match(/^.*$/gm)` returned `null` instead of
  `["one","two"]`, and CRLF (which is TWO terminators, with an empty line
  between them) reported `["two"]` instead of `["one","","two"]`, so any CRLF
  markdown, git output from a Windows checkout, or `/etc/os-release` parse
  silently mis-matched. The anchors are now spelled out against the same
  LineTerminator set #9218 gave `.`, sharing one definition so the two cannot
  drift; a multiline pattern with an anchor consequently compiles on
  `fancy-regex` rather than the linear engine.
