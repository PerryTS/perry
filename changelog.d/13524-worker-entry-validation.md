### Fixed

- Keep Worker filename kinds distinct from eval source during compilation and validate the original value before data transfer, using one runtime entry selector for compiled and unresolved file paths.
- Prepare Worker argv with Node-compatible conversion and observable option-read ordering, publishing one rooted child-local argv array without changing main-process arguments.
