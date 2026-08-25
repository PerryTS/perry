## Release engineering

- Publish all nine npm artifacts directly from GitHub Actions through npm
  Trusted Publisher/OIDC. The release path no longer needs an npm login, npm
  token, staged-package approval, or a local Socket credential.
- Pack once in the existing `release-packages.yml` workflow, optionally
  Socket-scan those exact tarballs, publish the same bytes, and verify their public
  registry shasums before creating the version tag and GitHub Release last.
