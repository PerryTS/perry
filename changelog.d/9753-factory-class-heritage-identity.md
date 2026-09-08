### Fixed

- Function-body class declarations with dynamic heritage now create a distinct
  class for each evaluation, even when their bodies capture no locals. Chained
  factories preserve their evaluated superclass and per-class static state.
  Prototype reflection, `instanceof`, and inherited method lookup follow each
  evaluation's own prototype chain instead of the shared template (#9502).
