Codegen tests for charter step 5 (P4): the raw-f64 class-field and typed
receiver-clone fixtures now initialize their number fields (`x = 0`) the way
real classes do. A number field that construction never writes is `undefined`
at birth, so it is correctly not an F64 lane; the assertions are unchanged.
