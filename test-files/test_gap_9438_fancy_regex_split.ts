// #9438: regex patterns that require fancy-regex used a separate split
// fallback which sliced between find_iter matches. That is not
// RegExp.prototype[@@split]: it emitted a trailing empty string for a match at
// the end and discarded every separator capture.

function row(name: string, value: string[]): void {
  console.log(name, JSON.stringify(value));
}

// Lookbehind and lookahead, with and without separator captures.
row("lookbehind/end", "a,b,".split(/(?<=,)/));
row("lookbehind/capture", "aXbXc".split(/((?<=a)X)/));
row("lookahead/middle", "abc".split(/(?=b)/));
row("lookahead/capture", "aXbXc".split(/(X(?=b))/));

// A zero-width match at either boundary must not open an empty chunk.
row("start", "abc".split(/(?=a)/));
row("end", "abc".split(/(?<=c)/));

// The empty-subject special case distinguishes a matching separator from a
// non-matching one.
row("empty/no-match", "".split(/(?<=a)/));
row("empty/match", "".split(/(?=)/));

// Captures count toward limit just like ordinary chunks.
row("limit", "aXbXc".split(/((?<=a)X)/, 2));

// #9427 rewrites multiline anchors to lookaround-bearing patterns, so these
// ordinary /m spellings also exercise the fancy lane.
row("multiline-start", "a\r\nb".split(/^/gm));
row("multiline-end", "a\r\nb".split(/$/gm));
