Prospective next experiment; not implemented or measured.

Current stringify_flat::string_piece declines any escape-triggering byte.
Consequently even an ordinary two-field object containing a large escaped
string loses direct final output and falls back to write_escaped_string's
Rust buffer followed by managed string creation. The slow writer loops over
every byte after the first escape, supports raw malformed/WTF-8 input and
performs repeated Vec extent/capacity operations. Preserve that complete
fallback, including surrogate pair normalization and lone-surrogate escapes.

Consider adding a native Piece variant for escapable, validated UTF-8 strings.
Keep the existing escape-free branch unchanged. For the new branch, validate
UTF-8 and count output expansion from ASCII quotes/backslashes/controls using
checked arithmetic, then plan exact bytes and UTF-16 units. Invalid UTF-8 or
WTF-8 declines to the existing serializer; no StringHeader flag may be assumed
trustworthy for validity or cached safety. SSO non-ASCII needs its existing
semantics or fallback. Strings can be appended in place; do not cache a safety
verdict in flags without complete mutation/propagation handling.

After the existing parent rooting, prototype proof and final allocation,
rederive source pointers and write escaped bytes directly into final output.
No intermediate managed allocation, full-size native buffer or second output
copy. Native plan fields are counts/lengths only. For valid UTF-8, non-ASCII
bytes can pass through; only ASCII escape expansion changes UTF-16 count.
Reserve/check all output bounds before allocation/emission; the final emission
must not collect or call user code. Avoid per-span memcpy calls for very short
runs if profiling confirms overhead; a raw byte writer or bounded chunks may
be preferable for escape-dense strings.

Tests need every ASCII byte at key/value/array positions, JSON short escapes,
Unicode mixtures, quotes/backslashes, retained exact output, UTF-16/byte lengths,
length overflow decline, malformed tails/WTF-8 fallback and forced collection
between planning and output. Existing prototype/getter/replacer/space behavior
must continue to decline or use the general path as appropriate. Full CPU/RSS
and all paired regressions remain required. This could address both the
escaped-stringify CPU deficit and its temporary-buffer memory cost.
