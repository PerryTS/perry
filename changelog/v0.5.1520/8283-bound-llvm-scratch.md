### Fixed

- Bound automatic LLVM failure retention to one diagnostic scratch directory
  per Perry process, preventing a failed multi-module compile from retaining a
  large IR copy for every module. Unix startup now also reaps scratch older
  than two hours when its owning process is gone, while preserving live and
  explicitly kept artifacts.
