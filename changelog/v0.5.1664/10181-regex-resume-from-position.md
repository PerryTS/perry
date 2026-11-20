### Performance

- **Non-ASCII RegExp `split`, `replace` and global `match` resume each search from where the previous one stopped** (#10164). On non-ASCII strings every search sought its start from an end of the string, which made these operations quadratic; they are now linear (log-log slope 1.02–1.05, from about 1.8). Capture strings are read from the match's position the same way. Requires `perex` 0.1.2.
