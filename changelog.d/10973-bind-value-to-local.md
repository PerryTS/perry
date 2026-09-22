### Internal

- `Expr::LocalSet`'s binding obligations now live in one function, `bind_lowered_value_to_local`: alias addref, closure captures and boxed cells, the canonical-i32 slot, the plain-slot store with its shadow-frame and i32 mirrors, module globals, arena owner, buffer views and int facts. Code that holds a value it didn't get from `lower_expr`, such as a read region's fast arm (#10884), can bind a local without re-implementing those steps, so none of them can be skipped. The body moved verbatim, and the LLVM IR is byte-identical on 41 `test-files/` programs.
