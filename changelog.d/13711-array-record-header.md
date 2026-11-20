- Reuse the validated arena header when acquiring an array iterator record,
  avoiding a repeated ownership lookup while preserving forwarding, sparse
  storage, and the existing non-arena validation path.
