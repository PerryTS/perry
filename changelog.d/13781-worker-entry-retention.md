### Fixed

- Preserve compiled file and eval Worker entries during dynamic module pruning and whole-program activation, including dynamic imports owned by the Worker. Reuse one module-root authority without disabling dead-import elimination.
