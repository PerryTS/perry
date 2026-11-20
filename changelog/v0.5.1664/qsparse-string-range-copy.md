Substring range copies now use the existing no-collection nursery allocator
before creating runtime handles. The common case copies from the validated
source directly; block exhaustion and large results share one rooted helper
that reloads the source after allocation. This removes repeated root-stack
work without changing generated call sites or adding retained state.

Coverage includes UTF-16 slice/substring bounds, lone surrogates, the absence
of a collection point on the open-block path, and source relocation on the
collecting path.
