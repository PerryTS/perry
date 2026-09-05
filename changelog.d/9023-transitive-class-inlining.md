### Fixed

- Keep cross-module helpers and methods that depend on imported classes in their source module, preserving constructors, methods, and iterators. Fixes the three failing codehz/ecs comprehensive performance tests (#9023).
