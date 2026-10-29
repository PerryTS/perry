Realm facts for 3,118 of 3,162 builtin declarations now live at their
`fn_info!` sites and in `realm_decl!` blocks beside the births that create the
objects, instead of in checked-in `*.realm.json` companions. The runtime build
producer reads those source declarations; the manifest it emits is identical to
the previous one after normalizing declaration source and line, and the hello
and lad-07-json binaries are byte-identical. Two optional declaration facts
were added: a `never_move` birth fact and an `integrity = frozen` callable fact.
