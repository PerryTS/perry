Generator intrinsic prototypes now use canonical property atoms and the existing
builtin prototype allocator, keeping protocol methods and iterator symbols in
inline shape slots. Sync and async generator property reads can use the ordinary
shape-proved inherited lookup while prototype replacement, deletion, and
Object.setPrototypeOf continue through the existing shape transitions.
