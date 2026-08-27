Correct the stale split regression expectation so an unproven `any` numeric
receiver is checked for ordinary missing-method `TypeError` semantics, while
the existing runtime string-dispatch coverage remains intact.
