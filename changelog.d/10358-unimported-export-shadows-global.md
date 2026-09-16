An un-imported export no longer shadows a global intrinsic.

A module that exports a name matching a global intrinsic made that name resolve
to the export at every call site in the program — even in modules that never
imported it. The global was shadowed by a binding the consuming module could not
see, so calls that should have reached the intrinsic reached the module's export
instead.

Resolution now requires the importing module to actually name the binding before
an export can win over the intrinsic.
