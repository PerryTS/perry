Generator intrinsic prototypes now use linker-shared property atoms and the existing
declared-layout prototype allocator, keeping protocol methods and iterator
symbols in inline shape slots without retaining unused old-generation room. Sync and async generator property reads can use the ordinary
shape-proved inherited lookup while prototype replacement, deletion, and
Object.setPrototypeOf continue through the existing shape transitions.
