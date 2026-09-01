Large pre-sized arrays now materialize their dense backing storage incrementally
during sequential indexed writes instead of falling back to quadratic sparse
property insertion. Growth also preserves expandos and GC-traced element slots.
