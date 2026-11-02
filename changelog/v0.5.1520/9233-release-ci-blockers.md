Budgeted garbage-collection cycles now build the lazy native stack-map index
before their first root scan, preventing traced workloads from aborting. The
`getDeviceModel` string-ABI regression also remains covered without requiring
a platform UI archive in runtime-only integration shards.
