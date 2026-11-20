### Changed

- A closure body that repeatedly reads a boxed capture it never assigns now resolves the box's cell address once at entry and loads the cell directly per read, instead of calling `js_box_get_bits` (a registry probe plus load) on every read. Mutations through other closures sharing the binding remain visible — only the never-moving cell address is cached, not the value.
