# Linux agent build storage

Repeated Rust builds need both a bounded MBX cache and compressed build storage.
Dedicated Linux agent hosts use `make build-agent`, which refuses a non-ZFS,
uncompressed, unbounded or deduplicated build dataset before invoking MBX.
Ordinary macOS/Windows contributors continue using `make build-dev`.

Provision a dedicated dataset in an **existing** pool as an administrator.
Choose the pool, mountpoint and quota for the host; this example reserves
a 200 GiB dataset quota. It does not create or reformat a pool.

```bash
sudo zfs create -o mountpoint=/srv/perry-builds -o compression=lz4 \
  -o dedup=off -o quota=200G tank/perry-builds
sudo install -d -o "$(id -un)" -g "$(id -gn)" \
  /srv/perry-builds/mbx /srv/perry-builds/targets
export MBX_CACHE_DIR=/srv/perry-builds/mbx
export MBX_TARGET_ROOT=/srv/perry-builds/targets
make mbx-deps
make build-agent
```

`zstd` (or an explicit `zstd-N` level) is also accepted. Keep store and
managed targets on the same dataset so eligible clones need not cross a
filesystem boundary. Use OpenZFS with block cloning enabled if reflink reuse
is required; compression alone does not prove cloning is available. Check
`zpool get feature@block_cloning tank` and the host's
`/sys/module/zfs/parameters/zfs_bclone_enabled`. The storage checker verifies
dataset placement and properties, not kernel reflink support.

Keep `CARGO_TARGET_DIR` and `CARGO_BUILD_BUILD_DIR` unset for this entry point:
MBX owns target placement. Set machine-specific cleanup budgets with
`mbx settings set gc.max_total_size 150GiB`, adjusted below the dataset quota
and available capacity. Logical and physical usage differ under compression;
monitor `zfs list -o name,used,available,logicalused,compressratio` and preview
cleanup with `mbx gc --dry-run`. Do not turn on block deduplication to chase
identical artifacts; use MBX and compression.

The PR lint job tests the storage rejection paths without requiring a ZFS
runner. Actual host provisioning remains an administrator operation.

Sources: [Infrastructure for Agentic Rust](https://blog.brokk.ai/infrastructure-for-agentic-rust/),
[MBX configuration](https://mr-boxington.jdx.dev/configuration), and
[OpenZFS dataset properties](https://openzfs.github.io/openzfs-docs/man/master/7/zfsprops.7.html).
