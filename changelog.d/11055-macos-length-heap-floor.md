### Fixed

Dynamic `.length` dispatch now recognizes pointer-backed values in low macOS
heap mappings instead of treating every address below 2 TiB as invalid.
