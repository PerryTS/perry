# Upstream reference

Perry's customized local action payload was reviewed against
`jdx/mr-boxington-action` commit
`2f127992584bb5b3bec99ad6731b35a5069043f7` (upstream version `1.7.1`). The
source pin is recorded in `upstream/actions/pins.json`. Workflows reach this
Perry-maintained copy only through `.github/actions/perry-mbx-cache`, which
fixes mbx 1.22.0 and Perry's release cache settings. `upstream/` is reference
metadata only and is not loaded at workflow runtime.
