`JSON.parse` no longer sizes every nested array of objects from the rest of the
input. Only the document's record array — the root, or a member of the root
object — takes that estimate, once per parse; other `[{...}]` arrays grow from
small, and an array that finished far below its reservation is copied to its
length. A 24 MB npm packument (a two-entry `signatures` array per version)
parsed into 4.2 GB and 2.7 s; it now takes ~150 MB and 0.1 s. Top-level and
`{"items": [...]}` record arrays keep their speed and memory.
