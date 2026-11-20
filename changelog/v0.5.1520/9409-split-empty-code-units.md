### Fixed

- **`split("")` splits into UTF-16 code units, so an astral character yields
  two parts (#9409).** §22.1.3.23 runs SplitMatch over the code-unit sequence,
  making `"😀".split("")` a two-element array of lone surrogates — matching
  `"😀".length === 2` and the halves `charAt(0)`/`charAt(1)` already returned.
  Perry stepped its WTF-8 payload one sequence at a time, so an astral
  character came back as a single part and every emoji-width, truncation and
  column calculation built on `split("")` saw one unit where Node sees two.
  Each half is now built with the same one-code-unit constructor `charAt` uses,
  keeping the `HAS_LONE_SURROGATES` flag so `isWellFormed()` and
  `JSON.stringify` still see a broken half; `limit` counts code units and may
  legitimately cut a pair.
