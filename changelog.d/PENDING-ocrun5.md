### Inherited static calls through fresh class evaluations

Static member calls now use the receiver-aware named property lookup for
inherited function and constructor-prototype properties. This removes the
separate template-parent-closure and Function.prototype fallback walks, so calls
follow the same pinned per-evaluation heritage as property reads, including
accessor receivers. No cache, registry, latch, or member-name exception is added.

The regression was exposed by #12191's 66b7275c follow-up: fresh evaluations
correctly stopped publishing their heritage into the shared class template, but
the static-call fallback still searched that template. A standalone Node parity
test covers a declared class above a fresh factory, inherited callable identity,
and distinct evaluation keys with the original call receiver.

The OpenCode witness is packages/server/src/location.ts:53:
LocationMiddleware.of throws before POST /session reaches session creation.
The SDK catches the fetch exception and run.ts:673 reports Session not found.
