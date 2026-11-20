Full and fallback garbage collections now process weak-reference holders from
their registry in budgeted slices instead of scanning the entire live heap in
one atomic pause.
