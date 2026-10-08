# Security policy

## Report a vulnerability privately

Use [GitHub private vulnerability reporting](https://github.com/PerryTS/perry/security/advisories/new)
to report a suspected security vulnerability to the maintainers. Private reporting
is enabled for this repository. Do not put exploit details, credentials, or other
sensitive information in a public issue or pull request.

Include the Perry version, operating system, affected command or API, a minimal
reproducer, and the security impact. For `perry publish`, include whether the hub
is hosted, remote self-hosted, or running on loopback. Redact tokens and secrets.

## Updates and previously published advisories

Use the latest stable [Perry release](https://github.com/PerryTS/perry/releases)
and review the [published security advisories](https://github.com/PerryTS/perry/security/advisories).
The versions below are the first patched versions listed in those advisories,
not a recommendation to stay on an old release.

| Advisory | Issue | First patched version |
| --- | --- | --- |
| [GHSA-x55v-q459-68ch](https://github.com/PerryTS/perry/security/advisories/GHSA-x55v-q459-68ch) (CVE-2026-53777) | `perry publish` arbitrary file write through a server-controlled artifact name | 0.5.1159 |
| [GHSA-5324-c68v-8w62](https://github.com/PerryTS/perry/security/advisories/GHSA-5324-c68v-8w62) | Native JWT verification accepted expired tokens | 0.5.1166 |

The publish fix rejects artifact names containing path traversal or path
separators and restricts the local-file copy shortcut to loopback hubs. Treat a
local hub as trusted: it can provide a local artifact path.

The JWT expiry fix was followed by removal of the native `jsonwebtoken` binding.
Current Perry compiles the npm package from source. Upgrade Perry and rebuild
executables compiled with affected versions; updating the compiler does not
change an executable already distributed. Keep the application's npm dependencies
up to date as well.

Published advisories remain visible on GitHub after fixes ship. Their presence
on the Security page is a historical record; consult each advisory's affected
and patched versions to determine whether an installation is affected.
