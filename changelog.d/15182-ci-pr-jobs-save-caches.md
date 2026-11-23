Only CI jobs that run on pull requests save Rust caches. Sweep, nightly and
cron jobs restore a pull-request family when their build matches one; their
own saves were newer than the pull-request families and evicted them from the
10 GB cache.
