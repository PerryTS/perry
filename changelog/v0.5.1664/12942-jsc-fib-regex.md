### Performance

- A parameter the specialized entry contract proves Number (and that is never rebound, mutated or boxed) no longer owns a GC root slot in the clone, so its reads skip the root reload. js-compiled `10-fib` is back at v0.5.1520's instruction count (5.63G → 3.91G instructions:u). (#12942)
- The budgeted GC step that every runtime poll runs (each regex capture copy, the microtask pump) answers "nothing due" from the #10698 trigger watermark before it samples the arena, and publishes the watermark when the trigger ladder finds nothing due. js-compiled `32-regex`: 211.5G → 144.4G instructions:u. (#12942)
