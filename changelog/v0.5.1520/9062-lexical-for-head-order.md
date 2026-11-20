### Fixed

- Multi-declarator `let` and `const` classic `for` heads now initialize bindings from left to right while preserving per-iteration closure captures, so later declarators can safely reference earlier ones.
