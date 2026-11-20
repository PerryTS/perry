### Performance

- Guarded `$spec_b` clones now call themselves directly when their recursive
  arguments constructively produce Numbers, avoiding the public parameter
  guard on every recursive edge. The proof is limited to canonical numeric
  constructions, so BigInt-capable arithmetic and annotation-only claims keep
  the guarded fallback.
