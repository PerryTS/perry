### Fixed

- **`gc-root-dominance-statepoints` could not run its checker at all**, and had
  failed on every `main` run and every PR since #8084 merged, with
  `node_modules/zod/src/index.ts is missing; run npm ci --ignore-scripts`.

  #8084 added the dependency-scale native corpus to the **statepoints** job.
  That corpus compiles `node_modules/zod`, but the `actions/setup-node` and
  `npm ci` steps that provide it lived only in the sibling `gc-root-dominance`
  job. The statepoints job therefore died during setup, before the root-store
  dominance checker ran.

  This is worse than a red X. The statepoints arm is the one covering the
  **shipped** lowering — RS4GC statepoints are the default on aarch64 and
  x86-64 — while the green sibling covers the shadow-frame arm. So for a day
  the gate looked like it was watching the default configuration and was
  watching nothing, and it red-lighted every open PR, which trains reviewers
  to ignore it. CLAUDE.md's hazards 2 and 4 at once.

  The job now installs node the same way its sibling does, pinned by
  `.node-version`, with `--ignore-scripts` for the same reason.
