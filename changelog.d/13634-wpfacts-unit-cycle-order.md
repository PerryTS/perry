Per-unit facts order the units of an import cycle by the edges whose values
they use. An edge that exists only through a parameter type or through a
namespace import's reach is weak; a key read statically off a namespace
(`ns.k`, `ns.a.b`) is a strong edge to the unit declaring it. The back edge of
a cycle, which the importer sees as unknown, is now a weak edge wherever the
strong edges have no cycle of their own, instead of whichever edge the
depth-first walk met first (tsc: `ExternalProject extends Project` saw
`Project` as unknown and broke the subclass). The order stays deterministic:
components in post-order, members in discovery order. A unit test covers a
cycle through a parameter type that was broken at `extends`.
