CI hygiene on main: the `-D warnings` build is clean again (dead snap_image items,
an unused `mut`, a duplicated test binding), the wasm EH test builds `Stmt::Try` with
`forof_completion`, `snap_image_literal_order` is classified for e2e scoping, files
over the 2000-line cap are split into sibling modules, and the tree is rustfmt-clean.
