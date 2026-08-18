Add a local staged-publish pipeline, ported from the fleet-style staged-publish
publish architecture and made Perry-centric. Publishing was previously
CI-only (`release-packages.yml`) with no approve gate and no pre-publish
tarball scan; this adds `npm run publish:*` scripts that stage, verify,
socket-scan, and — after a human approve gate — promote + cut the GitHub
release.

**New `scripts/publish/` tree** (generic core + npm/brew/cargo tiers):

- `pipeline.mts` — orchestrator: `publish:stage` (dispatch the new
  `npm-stage-publish.yml` CI workflow under OIDC) → verify (local `npm pack`
  sha1 vs staged shasum) → Socket full-scan → `publish:approve` (browser
  web-OTP 2FA `npm stage approve`) → `publish:release` (tag + immutable
  GitHub release, draft→upload→undraft, behind a registry-liveness gate).
- `scan.mts` — Socket full-scan of each staged tarball via
  `@socketsecurity/sdk` `createOrgFullScanFromArchive`; `error`-action alerts
  (per the org's own security policy) fail the gate, `warn`-action alerts pass
  with counts. Fail-closed on unreachable/empty scans.
- `npm/{staged,approve,publish-command,pack via staged,shared,bump}.mts` —
  the `npm stage publish`/`npm stage approve` mechanics, the shasum verify
  gate, and Perry's version source (Cargo.toml + CLAUDE.md `Current Version`
  agreement + `changelog.d/` fragments + tag-not-already-existing — the same
  STOP conditions `release-packages.yml` enforces, surfaced locally so a bad
  dispatch fails in seconds).
- `auth-posture.mts` — refuses any long-lived `NPM_TOKEN`/`NODE_AUTH_TOKEN`/
  `NPM_AUTH_TOKEN` on publish (OIDC-in-CI + 2FA-locally only). A read-only
  `PERRY_NPM_READONLY_TOKEN` powers registry reads and can never publish.
  `prepublishOnly` is wired to this guard.
- `brew/{formula,tap-publish}.mts` + `cargo/ffi-publish.mts` — locally-runnable
  brew tap bump (render `Formula/perry.rb` from release coordinates + per-asset
  sha256, push to `PerryTS/homebrew-perry`) and the perry-ffi → crates.io
  publish (perry-runtime-first order preserved).
- `release.mts` uploads `packaging/install.sh` + `checksums.txt` as per-tag
  release assets, so `curl -fsSL …/releases/download/vX.Y.Z/install.sh | sh`
  works per tag.

**New CI workflow** `.github/workflows/npm-stage-publish.yml`: an OIDC staged
upload (build legs reused from `release-packages.yml` stage mode →
`stage-npm.sh` → `npm stage publish --provenance` for all 9 packages,
`environment: npm-publish`, `id-token: write`, no long-lived token). Stages
ONLY — nothing is public until the local `publish:approve`. The existing
`release-packages.yml` `npm-publish` job stays as the republish/emergency
fallback.

**Prerequisites the author provisions** (documented in-script): npm staged
publishing enrolled for `@perryts/*`; the new workflow added as a trusted
publisher on each package; `SOCKET_API_TOKEN` for scans; `HOMEBREW_TAP_TOKEN`/
`APT_REPO_TOKEN` for tap pushes; `PERRY_NPM_READONLY_TOKEN` for registry reads.

Tests: `scripts/publish/publish.test.mts` covers the formula renderer, policy
bucketing, human-gate shape, and the auth-posture refusal (sabotage-tested:
the refusal is asserted with a long-lived token present and the clean path
with it absent).
