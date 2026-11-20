### Fixed

- **The lightweight GC root-holder, thread-local, and ratchet audits now run on
  Windows** (#7878). Repository-relative scanner keys use stable forward-slash
  paths, the ratchet's structural validator no longer imports a Unix-only
  module, and the Windows CI job exercises both scanners plus structural
  ratchet validation before starting its compiler build. The measurement path
  still requires Unix `os.wait4` so its per-process peak-RSS readings cannot
  silently degrade.
