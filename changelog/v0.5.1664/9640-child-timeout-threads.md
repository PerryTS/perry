Fixed timed `child_process.spawn`, `exec`, and `execFile` calls retaining
one sleeping OS thread until the full timeout after a child had already exited.
Timeout workers now wake on child completion while preserving deadline kills.
