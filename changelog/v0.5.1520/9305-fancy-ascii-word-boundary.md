### Fixed

- A `RegExp` combining a lookaround (or backreference) with `\b`/`\B` no
  longer throws a bogus `SyntaxError: invalid pattern`: the ASCII
  word-boundary spelling `(?-iu:\b)` the translator emits (#9263) is valid
  for the linear engine but rejected by fancy-regex's parser, so every
  pattern forced onto the fancy engine lost its word boundaries.
  `build_fancy_regex` now rewrites the marker into the equivalent
  one-code-point-lookaround form. This was the throw inside a microtask
  that #9305's setjmp miscompile turned into the `cc --help` segfault;
  with both fixed, `marked`'s html-block regex — and `cc --help` — work
  again. (#9305)
