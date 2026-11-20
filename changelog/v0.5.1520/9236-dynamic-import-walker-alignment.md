Dynamic imports inside closures and top-level dynamic imports now keep their
resolution outcomes aligned, preventing valid literal imports from being
lowered as runtime-computed import errors.
