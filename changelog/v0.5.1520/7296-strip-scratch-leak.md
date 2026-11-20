`perry compile` created `perry_strip_<pid>/` under the system temp directory at
eight sites and never removed it. The `_extract` subdirectories were cleaned; the
parent — holding every `_<lib>_trimmed.lib` — was not, so one directory leaked per
compile, at roughly 64 per two hours of ordinary activity.

They twice took a development machine to zero bytes free, which surfaces as
unrelated build failures in every concurrent process rather than as a disk error
at the leak site: it killed one background agent outright and stalled another.

All eight sites now share one `strip_tmp_base()`, which sweeps *other* processes'
dead-PID directories on first use. The sweep is startup-time rather than an exit
hook deliberately — it also heals crashes, `SIGKILL` and `process::exit`, none of
which run destructors — and it never touches a live PID's directory, so
concurrent `perry` invocations remain safe.
