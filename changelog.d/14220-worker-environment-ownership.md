### Fixed

- Worker environments use construction-time private copies or explicit SHARE_ENV backing without process-wide OS swaps. Invalid environment options reject before transfer, aliases and child-process defaults follow the effective environment, and warm cached environment reads remain allocation-free.
