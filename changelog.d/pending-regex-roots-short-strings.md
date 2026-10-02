### Faster

- Non-global, non-sticky regex calls skip position-hint lookups and writes, since they always search from zero. A numeric `lastIndex` also avoids unobservable offset calculation; other values retain their coercion effects and exceptions (#10166).
- Short ASCII split pieces, exec captures and callback-replacement arguments use inline strings instead of allocating one heap string each. Longer and non-ASCII strings retain the exact UTF-16 copying path.
- A builtin regex split copies its capture spans directly into the output instead of constructing a complete exec result array for every separator. Unmatched captures, limits, named groups and lookaround captures retain their JavaScript behavior; custom species and exec overrides keep the observable path.
