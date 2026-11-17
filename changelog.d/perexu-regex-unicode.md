### Changed

- Select Perex Unicode property membership tables only when regex patterns or open runtime producers can require them; preserve dynamic RegExp construction and Unicode matching.
- Resolve selectable Unicode membership during compilation or validation, cache its native provider through program relocation, and preserve typed witness storage for heap scanning.
