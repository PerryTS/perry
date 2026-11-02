Speed up calls to `ReadonlySet.has` with an exact runtime Set-brand check and
ordinary method dispatch fallback. Type-only class imports now retain the
field metadata needed for this guarded optimization without creating runtime
module bindings or initialization edges.
