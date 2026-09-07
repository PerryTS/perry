### Performance

- Native method dispatch now classifies managed receivers from their NaN-box tag and allocator-proven GC type instead of consulting the Buffer and typed-array registries for every call. Typed-feedback sites retain a small class/type cache, while external buffers, shared backings, and headerless native typed views keep registry fallback semantics.
- `Intl.Segmenter` view entry points validate their fixed cursor brand with an arena/type/class-id load, and the RegExp view test no longer repeats a pointer validation already performed at the exported boundary.
