A `require()` of a `.node` addon built for another target triple was still
followed as a module edge, so cross-compiles tried to resolve addons the host
cannot use. Such edges are now skipped when the addon path does not match the
host triple.
