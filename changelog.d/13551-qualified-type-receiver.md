A receiver typed `Path.Path` (a declaration behind a namespace or `import *
as` binding) could not be matched against the class and interface registries
and fell through to the array assumption, so `path.join(...)` folded into
`Array.prototype.join`. Qualified type names are no longer assumed to be
arrays.
