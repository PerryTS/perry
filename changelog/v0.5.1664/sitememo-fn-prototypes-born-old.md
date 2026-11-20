### Changed

- Every function default prototype (`ensure_function_prototype_object`) is
  born old and never-move, a young function's included. The class-prototype
  table roots it for the rest of the process, so a young birth only delayed a
  certain promotion, and until that promotion every site memo answering
  through it was refused as a young referent.
