### Changed

- Boolean helpers with erased numeric parameters now use a guarded typed clone
  for comparison-heavy predicates, avoiding repeated generic relational calls.
