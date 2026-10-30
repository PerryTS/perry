RegExp `exec` and `test` read the receiver's matcher data and its `lastIndex`
once per operation, from the RegExp birth layout's own inline slots, instead of
re-reading each through the keyed brand lookup and `Get(R, "lastIndex")`. Any
other layout (a subclass, an added own key, an accessor `lastIndex`) keeps the
keyed read and the ordinary `Get`, and the data is read again after a non-Number
`lastIndex` ran user code, so a `valueOf` that recompiles the receiver is still
observed. A gap test checks the semantics against node.
