### Changed

- `sub.push(v)` on a `class X extends Array` instance takes the inline append tier: the receiver's meta record resolves the elements store and the ordinary room/integrity tests and inline store run on it, instead of calling the runtime entry whose only job was to follow that pointer. Growth, forwarded receivers and every exotic flag keep the runtime path.
