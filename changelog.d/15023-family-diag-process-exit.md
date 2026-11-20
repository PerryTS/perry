PERRY_FAMILY_DIAG now also prints its family store report when the program ends through process.exit (the atexit path was skipped), at the cost of one relaxed load when the variable is unset.
