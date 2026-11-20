A typed array whose tracked view was demoted (it escaped to an unknown or
dynamic call, a spread or `Reflect` call) now reads its elements through the
guarded inline load instead of a `js_typed_array_get` call plus a
`js_number_coerce` per read. A view whose `.buffer` was exposed keeps the
call, since its elements live out of line.
