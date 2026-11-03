Generator intrinsic prototypes now use linker-shared property atoms and the
existing intrinsic-prototype allocator with room for their fixed properties.
Protocol methods and iterator symbols stay in inline shape slots without
retaining unused old-generation room. Sync and async generator reads use the
ordinary shape-proved inherited lookup while overrides, deletion, and
Object.setPrototypeOf retain the existing shape transitions.
