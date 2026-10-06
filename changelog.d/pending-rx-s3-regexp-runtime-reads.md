RegExp builtins now read exec, flags, constructor and symbol methods through
ordinary runtime read sites. Replacing a flag getter is observed even when its
descriptor attributes and holder shape stay unchanged. The remaining
flags_reads_getters observability row now passes.

Deletes the separate RegExp canonical proof, recorded test-method site and
flag/symbol epoch checks. Runtime read sites support non-observable accessor
and symbol probes using the existing holder entries and accessor-lane guards.
