`Array.prototype.splice` on a frozen array (direct or through
`Reflect.apply`/`.call`) moved, inserted and removed elements instead of
throwing. A frozen array's elements and `length` are non-writable, so every
splice, `splice(0, 0)` included, now throws a TypeError before the array is
touched.
