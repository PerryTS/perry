# GitHub Actions consolidation implementation plan

> **Current status (2026-10):** the initial rollout below is historical. All 38 suites are now inlined under six repository entrypoints: CI, Extended Tests, GC, Repository, and the two publishing workflows. Extended Tests groups compiler/runtime, compatibility, integration, and performance; Repository groups documentation and maintenance. GitHub's four dynamic registrations bring the sidebar total to ten after retired run history is cleaned up. See `docs/src/testing/ci-tiers.md` and the live `scripts/actions_catalog.json` for current routing; the frozen inventory and implementation instructions below describe the original ten-parent rollout.

Implement 10 category entrypoints with related suites shown as nested jobs. This is a handoff for `gpt-6-luna` with low reasoning and the user's Fast setting. Follow the numbered phases in order. The plan supplies the architecture, filenames, routing rules, exceptions, validation cases, and completion criteria; do not redesign these during implementation.

Read `CLAUDE.md` in full before work. This plan covers workflow implementation and preparation for review. Its rollout section describes a later operator task; implementing the plan does not itself authorize publishing packages, merging, cancelling runs, or deleting run history.

## Baseline and known failures

- Repository: `/Users/jdalton/projects/perry`, remote `origin` is `PerryTS/perry`.
- Inspected commit: `5423c426f4066c666aa3311672aef270ff69f3ae` on 2026-10-07.
- Source inventory: `docs/audits/actions-consolidation-inventory.json`. It contains the original triggers, inputs, environment, concurrency, permissions, and job contracts for all **38** checked-in workflow files. Earlier conversation estimates of 37 omitted `npm-launcher.yml`.
- Baseline release failure: <https://github.com/PerryTS/perry/actions/runs/37689359118>. GitHub reports `Line: 588, Col: 7: A sequence was not expected`. An action invocation was inserted into `jobs.build.strategy.matrix` instead of `jobs.build.steps`.
- `gc-native-roots.yml` already has `name: gc-native-roots`, after its long header comment. Do not add a duplicate name.
- GitHub lists seven deleted diagnostic workflows as registered: `diag-8117.yml`, `hang-lab.yml`, `llvm-inprocess-feasibility.yml`, `probe-10058.yml`, `probe-fetch.yml`, `probe-h2hang.yml`, and `zlib-debug.yml`. Do not recreate them.
- Copilot, Dependabot Updates, Dependency Graph, and `pages-build-deployment` are GitHub-managed registrations. Do not attempt to implement or consolidate these in repository YAML.

The inventory is a historical migration reference, not a live workflow generator. If latest `origin/main` has changed a source file's SHA256, refresh that source's inventory row and review its contracts before transforming it. Do not overwrite new source changes using the snapshot.

## Required final topology

Only the following 10 files may have independent event triggers. Other workflow files in the mapping must have `workflow_call` only. A category consists of one ordinary Actions run; reusable suites appear as named jobs inside that run. Do not dispatch each child using the REST API: that would create separate independent runs again.

| Entrypoint file | Display name | Existing implementations it owns |
| --- | --- | --- |
| `test.yml` | `CI` | Existing core jobs, `coverage.yml`, `security-audit.yml`, `zizmor.yml`, `native-result-ledger.yml`, `npm-launcher.yml` |
| `gc.yml` | `GC` | `gc-ratchet.yml`, `gc-moving-witnesses.yml`, `gc-native-roots.yml`, `gc-root-dominance.yml`, `gc-parse-churn-gate.yml`, `gc-ptr-shape-off-witness.yml` |
| `compiler-runtime.yml` | `Compiler and Runtime` | `llvm-inprocess.yml`, `cc-parity.yml`, `eh-transport.yml`, `ext-link.yml` |
| `compatibility.yml` | `Compatibility` | `node-compat-matrix.yml`, `node-suite-guard.yml`, `node-core-subset.yml`, `feature-matrix.yml`, `wasi-check.yml`, `npm-package-sweep.yml` |
| `integration.yml` | `Integration` | `container-tests.yml`, `simctl-tests.yml`, `next-app-route.yml`, `auto-opt-app-patterns.yml` |
| `performance.yml` | `Performance` | `benchmark.yml`, `tak-performance.yml`, `tls-budget.yml` |
| `documentation.yml` | `Documentation` | `docs-check.yml`, `docs.yml` |
| `release-packages.yml` | `Release / Packages` | Existing implementation stays in this file |
| `release-hono-server.yml` | `Release / Hono Server` | Existing implementation stays in this file |
| `maintenance.yml` | `Maintenance` | `ci-queue-reaper.yml`, `gate-freshness.yml`, `gate-failure-watch.yml`, `soak-autofix.yml`, `npm-publish-freshness.yml` |

Retain the existing child filenames, job IDs, existing explicit job names, matrices, artifacts, cache keys, tool versions, scripts, and module environment. This avoids unnecessary contract changes. There are seven new category YAML files; `test.yml` and the two release files remain entrypoints. Keeping reusable child files is intentional: 10 event entrypoints is the goal, not 10 YAML files total.

Caller job IDs and display names must both equal the child's basename without `.yml`, for example `gc-ratchet`, `simctl-tests`, and `node-core-subset`. This gives stable nested names such as `gc-ratchet / gc-ratchet`. Preserve the existing `security-audit` caller job name in CI. For the auxiliary security run use `security-weekly`, as described below.

Use category run titles that expose the selection and ref, for example `GC / gc-native-roots / main` for a single scheduled suite and `GC / all / feature-branch` for a manual family run. Do not interpolate untrusted titles or inputs directly into shell commands. The router should also write a summary listing the selected suites, event, ref, and schedule.

## Fixed implementation files

Create these implementation files during the later implementation task:

- `scripts/actions_catalog.json`: explicit category ownership, original event rules, child input definitions, and parent input mappings. Populate event rules from the inventory, not from converted child headers.
- `scripts/actions_plan.py`: offline event router; standard library only. It reads the catalog and emits string outputs usable by Actions.
- `scripts/test_actions_plan.py`: focused routing and contract tests using synthetic event payloads. These tests are required because consolidating independent triggers can silently enable or disable checks.
- `scripts/check_actions_topology.py`: read YAML with the same PyYAML installation used by existing workflow audits. Check the 10-entrypoint allowlist, unique ownership, declared calls/inputs, catalog schedules, and no surviving child event triggers. Make it runnable from the existing CI lint job.

Use catalog version `1`. Each category has `name`, `entrypoint`, and a `modules` array. Each module has `id`, `file`, `original_events`, `inputs`, `parent_input_map`, and `pr_label` when relevant. CI also has the special `core` route described below. Do not invent a general workflow framework or generate entire YAML files with a YAML serializer.

Edit existing YAML surgically. Serializers can rewrite shell blocks, quotes, or the key `on`, and make a large unrelated diff. Keep both the inventory JSON and runtime catalog valid JSON with two-space indentation.

## Event routing contract

The router CLI is:

```text
python3 scripts/actions_plan.py --category <catalog-key> --event <event-name> \
  --event-path <GITHUB_EVENT_PATH> --ref <GITHUB_REF> \
  [--changed-files <newline-separated-file>] [--suite <suite-id>]
```

Outputs: `plan` is compact JSON mapping every suite ID in the category to a boolean, `selection` is a printable ordered list of selected suite IDs, and CI additionally emits `run_core` as `true` or `false`. Append outputs to `GITHUB_OUTPUT` when it is set; print the same JSON for offline use. Validate event payload shape and manual selections; an unknown category, suite, or cron must fail visibly.

Every category entrypoint gets one small `route` job with `contents: read` and `pull-requests: read` where required. Use the repository's existing inline git checkout pattern with credentials not persisted. For PR diffs, copy the `scripts/ci_pr_files.py` invocation from CI, including expected-head checking. For push path filters, use an explicitly fetched `before..after` diff; on a missing/zero base or unavailable diff, conservatively select relevant push suites rather than treating the diff as empty. Never calculate PR changes against an arbitrary current `main` tip.

The router selects suites according to their **original** event rules:

1. `schedule`: match `github.event.schedule` exactly to the module's original cron. Select only modules owning that cron. When two modules share a cron, select both. Preserve all existing cron strings and staggered timing in this migration; do not launch all six-hourly gates at once.
2. `pull_request`: honor each module's original branch filters, event types and path filters. Add the suite's parent entrypoint, catalog/router files, and that child file to its relevant workflow-change paths. A parent having a broad PR trigger must not make every child run on every PR.
3. Preserve `run-extended-tests` and `run-cc-parity` gating. Existing child job conditions remain authoritative. Route eligible unlabelled PR suites where they contain unconditional lightweight checker jobs; do not remove those self-tests accidentally. Heavy child jobs must remain skipped without their existing label.
4. `push`: match the module's original branch/tag patterns and path filters. A module with no original push trigger is not selected for pushes merely because its parent has one.
5. `merge_group`: select only the Node compatibility matrix and Node suite, which already have this event. Preserve the original semantics.
6. `release`: select only modules whose original rules include the release action. In Performance this is `benchmark`; it must not automatically run `tak-performance` or `tls-budget`.
7. `workflow_dispatch`: use the category's `suite` input. `all` selects the category's manually runnable modules; an individual ID selects just that module. Reject invalid IDs. CI and Maintenance use the special defaults below.
8. `workflow_run`: Maintenance selects only the failure observer. Apply the trusted upstream event/branch checks already used by `gate-failure-watch.yml`.

Parent trigger headers are the union of their modules' original events. For PR types, retain `opened`, `synchronize`, `reopened`, and `labeled` where used. A source PR trigger with no `types` uses GitHub's defaults `opened`, `synchronize`, and `reopened`; do not start its suite on `labeled` simply because a sibling subscribed to it. A module with an unfiltered PR/push trigger requires a broad parent trigger; otherwise use the union of path filters. The router still applies each original module rule. Keep `branches: [main]` where the original category rules all require it, and keep original tag filters rather than broadening to all tags.

Pattern matching must support the patterns actually present in the inventory: `*`, `**`, `?`, and character classes, plus ordered `!` exclusions if any appear after baseline refresh. Single `*` must not cross `/`; `**` may cross directories, and `**/` may match zero directories. Match complete paths. Do not silently use Python `fnmatch` for GitHub paths: its `*` crosses `/`. The original container rule `types/perry/{container,compose,workloads}/**` should be explicitly represented as three path patterns in the catalog; test each intended directory.

Caller job shape for normal read-only suites:

```yaml
  gc-ratchet:
    name: gc-ratchet
    needs: route
    if: needs.route.result == 'success' && fromJSON(needs.route.outputs.plan)['gc-ratchet']
    permissions:
      contents: read
      pull-requests: read
    uses: ./.github/workflows/gc-ratchet.yml
```

Do not put `runs-on`, `steps`, or `env` on a caller job containing `uses`. Child jobs do their own checkouts. Parent workflow environment is not propagated to reusable workflows; preserve each child's environment inside that file. Forward only the secrets needed by that module, rather than adding `secrets: inherit` everywhere.

The caller must allow all permissions its called jobs require; a child cannot increase the caller's token permissions. Give each caller the minimum compatible permissions. Preserve named environments such as `github-pages` inside child jobs.

Parents use a category-specific concurrency prefix, supersede PR runs only, and key other events by `github.run_id`. Children keep suite-specific concurrency prefixes. Caller and child must never use the same concurrency group, because their GitHub context identifies the same parent run. Preserve CI's intentional coalescing of main push sweeps.

## Phase 0 Prepare the implementation branch

1. Read `CLAUDE.md` fully and find any applicable nested `AGENTS.md`.
2. Inspect `git status`; preserve unrelated work. Fetch `origin/main`. Fast-forward a clean main checkout, or create a separate branch/worktree from the fetched commit if local changes prevent updating safely.
3. Use branch `chore/actions-consolidation` or a unique suffix if that branch already exists.
4. Compare every inventory SHA256 with its current workflow file. Refresh changed rows and the baseline SHA. Stop for a design review if a new workflow, changed publisher identity, changed required gate, or changed category ownership invalidates this mapping. Routine source updates can be incorporated directly.
5. Record the exact baseline revision and known existing validation failures in the implementation notes. Do not classify a workflow as nameless from its first 50 lines; inspect its full YAML.

Exit criterion: 38 baseline files are accounted for, or a refreshed mapping explicitly accounts for newer files, and no unrelated work is lost.

## Phase 1 Repair the release syntax

In `release-packages.yml`, change `jobs.build.strategy.matrix` back to an object containing `include`. Remove the misplaced action list item and its `with` from that location. Preserve every existing matrix include row byte-for-byte.

Inspect `jobs.build.steps` and `prime-macos-x86_64-cache.steps` before inserting anything: the cache action may already exist in the correct location. Keep the cache warmer's existing invocation. Add a build-step invocation only if the build job actually needs the same missing invocation; do not duplicate it. This repair must never leave an action object under a matrix.

Keep the action inputs and cache generation consistent with the actual target. Do not apply `perry-release-x86_64-apple-darwin` to Linux, Windows, or ARM macOS matrix rows. A new build invocation intended solely for Intel macOS needs an explicit `matrix.target == 'x86_64-apple-darwin'` condition.

Validate syntax and job dependencies before proceeding. The result must retain the build matrix, `continue-on-error` on the optional cache warmer only, and the preflight/await-tests requirements. This small repair can be reviewed as its own first commit.

## Phase 2 Implement and prove the router

Populate the catalog with all mapped source events from the inventory. Add the input aliases below, preserving defaults and types. Add tests before replacing independent trigger headers. Use synthetic files/events; no Rust builds or remote workflow dispatches are needed.

| Category | Manual input contract |
| --- | --- |
| CI | `suite` choice: `core` default, `all`, `coverage`, `security-audit`, `zizmor`, `native-result-ledger`, `npm-launcher`; retain `tier` and `update_gap_snapshot` unchanged |
| GC | `suite`: `all` default or any of the six GC suite IDs |
| Compiler and Runtime | `suite`: `all` default or any of its four suite IDs |
| Compatibility | `suite`: `all` default or a module ID; `apis` string `''`; `max_per_api` string `'25'`; `packages` string `''`; `limit` number `6`; `strict` boolean `false` |
| Integration | `suite`: `all` default or a module ID; `device` string `'iPhone 15'`; `run_e2e` and `run_fuzz` retain string choice `'true'`/`'false'` with default `'false'` |
| Performance | `suite`: `all` default or a module ID |
| Documentation | `suite` choice: `docs-check` default, `docs`, `all`; `docs` explicitly means deploy |
| Maintenance | `suite` choice: `validate` default, `ci-queue-reaper`, `gate-freshness`, `soak-autofix`, `npm-publish-freshness`; `apply` boolean `false`; no writable `all` default |
| Both Release workflows | Existing inputs unchanged |

`all` means a whole category in one run. Preserve internal child choices: manually selecting containers does not imply e2e/fuzz enabled, and selecting Documentation checks does not deploy. Explicit Documentation `all` includes deployment.

The implementation must convert every child use of `github.event.inputs.<field>` to its declared reusable `inputs.<field>`, including container choices, simulator device, and package sweep options. Parent aliases preserve original types. Do not use string boolean truthiness or `value || fallback` when valid `0` must survive; `limit=0` and `max_per_api='0'` are meaningful.

Required router cases are listed in Phase 7. Exit criterion: all routing tests pass using original headers before conversion.

## Phase 3 Consolidate normal category suites

Implement GC, Compiler and Runtime, Compatibility, Integration, Performance, and Documentation. For each child in those categories:

1. Replace `on` with `workflow_call` and the typed inputs it consumes. Remove its independent schedule, dispatch, PR, push, release, or merge-group headers.
2. Preserve its existing module environment, job IDs, job names, run conditions, needs, matrices, artifact names, liveness assertions, permissions, and script commands. Reusable workflows retain the originating caller event in `github.event_name`; do not replace every event condition with `workflow_call`.
3. Keep existing PR diff checks inside the child; update their workflow-file regexes to include the parent entrypoint and shared routing/catalog paths. The router's external filter and the child's internal relevance filter must both recognize changes to the parent.
4. Add its parent caller job with a static name equal to the module ID, routing condition, minimum compatible permissions, and explicit input mappings.
5. Add the parent event union and every original cron verbatim. Use the prescribed independent parent concurrency prefix.
6. Check that no suite runs twice on one event, once from a parent and once from a surviving child trigger.

Category exceptions:

- Performance `benchmark` supports `release: published`, tag pushes, schedule and dispatch; retain the existing `IS_RELEASE` behavior. `tak-performance` has a `contents: write` publication job and `actions: read`; allow those on its caller without granting them to TLS or benchmark jobs. Do not change report-only tak measurements into a required gate.
- Documentation's `docs` caller needs `pages: write` and `id-token: write`. Its environment and `pages` child concurrency stay intact. Documentation validation callers receive read-only permissions. Deployment remains selected only for a matching release tag or explicit manual deploy selection.
- Integration simulator originally cancels by ref. To stop one release verification from cancelling another, change this child's concurrency to cancel PR events only and key non-PR events by run ID; keep its `simctl-` prefix. Record this intentional correction in the changeset.
- Container and coverage workflows have unconditional cancellation in the baseline. Correct non-PR cancellation while migrating: supersede only PR events. Keep their matrix `fail-fast: false` contracts.
- Compatibility's Node suites retain `merge_group`. Node core and npm package sweeps remain advisory nightly/manual workloads, not new PR workloads.
- Node-core's current named shard jobs and report fan-in must remain intact; never make a missing shard look like a successful complete report.

Exit criterion: these six parents own exactly 25 child implementations, every corresponding child is call-only, and both original triggers and module options are represented in the catalog. Count 6 GC + 4 compiler + 6 compatibility + 4 integration + 3 performance + 2 docs.

## Phase 4 Consolidate CI auxiliaries without changing the core gate

Keep the existing core implementation in `test.yml`. Do not move it behind another reusable caller, because that would change the exact `pr-gate` and `full-suite-gate` names consumed by branch protection and release polling.

Add a `route` job to CI and expose its `run_core` output. Keep the existing `plan` job ID and its `plan`/`tier` outputs. Give `plan` `needs: route` and run it only when routing succeeds and `run_core == 'true'`. Existing core jobs continue depending on `plan` and on their current planner booleans.

Compute `run_core` as follows:

- Every `pull_request` event: true, including docs-only PRs and labelled events.
- Existing core pushes to main or `v*` tags: true.
- Existing CI crons `0 4 * * *` and `47 */2 * * *`: true.
- Dispatch `suite=core` or `suite=all`: true. Retain existing `tier` and snapshot behavior.
- Coverage's weekly cron, security's weekly cron, and dispatch of an auxiliary suite: false.

Add `route` to the existing gate's `needs`. Its condition becomes `always()` plus `(event is pull_request OR run_core == 'true')`. On PRs, a failed route still makes the gate run and fail through its required route/plan checks. Require both route and plan to succeed for core verdicts. On auxiliary-only scheduled/manual runs, skip the core gate rather than emitting a misleading `main-gate` or `full-suite-gate`.

Preserve the gate's existing dynamic name expression and its existing needs list. Do not make an auxiliary manual run with `tier=full` produce `full-suite-gate`. The no-core scheduled jobs must not accidentally invoke `ci_plan.py` with an unrecognized cron and thereby run an extra sweep.

Convert coverage, zizmor, native-result-ledger and npm-launcher to call-only. Route their original events/paths and dispatch selections. Retain the npm launcher's read-only token and nonpersisted checkout credentials; its Ubuntu published-package smoke remains manual-only.

Security is special:

- Convert `security-audit.yml` to call-only by removing weekly schedule and manual dispatch; it already has `workflow_call`.
- Keep the existing core caller job ID/name `security-audit`, current plan conditions, and inclusion in the core gate.
- Add a separate caller job ID/name `security-weekly` for the original weekly cron `0 12 * * 1` or manual `suite=security-audit`, but select it only when `run_core == 'false'`. For `suite=all`, the existing core security caller supplies the audit, avoiding a duplicate.
- In the baseline, Security Audit now has actual job IDs `security-audit`, `soak-gate`, and `config-scans`. The freshness manifest still lists the obsolete names `agent-scan`, `skills-scan`, and `cargo-deny`. Correct the manifest to actual executed names during Phase 6; do not restore deleted jobs just to satisfy the old list.

Exit criterion: normal CI core events produce exactly the established core gate, auxiliary-only events run their suites without a core gate, and release dispatch `test.yml -f tier=full` behaves as before because `suite=core` is the default.

## Phase 5 Consolidate Maintenance with separate trust boundaries

Create `maintenance.yml` with the union of current maintenance schedules, relevant PR paths, manual choices, and a `workflow_run` observer trigger. Its global permissions should be read-only; writable permissions belong to individual jobs.

Place the existing **PR validation jobs directly in the parent**, not inside a writable reusable workflow call. A reusable child containing write-permission jobs can require caller permission even when its writable job is conditionally skipped. Avoid solving that by granting writable tokens to PR calls.

Copy the original offline validation commands and read-only checkout steps into parent jobs:

- `freshness-validation`: `python3 scripts/check_gate_freshness.py --self-test`.
- `observer-validation`: `python3 scripts/gate_failure_watch.py --self-test` and `--check-config`; install PyYAML as in existing validation dependencies.
- `npm-freshness-validation`: `python3 scripts/check_npm_publish_freshness.py --self-test` and `--check-manifest`.
- `routing-validation`: the new routing/topology checks.

For PRs select each original validation according to its original paths; shared router/catalog or Maintenance edits select the corresponding routing and impacted maintenance validations. Manual `suite=validate` runs all these offline checks and has no live issue writes, queue cancellation, or bot-branch mutation.

Convert the five old maintenance files to call-only implementations. Remove validation jobs now copied into the parent, retaining the original writable runtime jobs and guard conditions. Forward `apply` explicitly to the queue reaper, changing its expression to use the reusable input. Scheduled queue cleanup still applies as before; manual cleanup with `apply=false` remains dry-run.

Add a parent rejection job for manual dispatch of live freshness or soak-autofix on any ref other than `refs/heads/main`. The rejection must fail clearly rather than quietly produce an all-skipped green run. Queue dry-run can run on a topic branch; writable queue apply must use main. Do not add a default manual command that writes to external services.

Writable callers are selected only for their original schedule, explicit main dispatch, or trusted observer event. Give each caller exactly the needed permissions: issue writers `issues: write`/`actions: read`, queue reaper `actions: write`/`pull-requests: read`, soak bot `contents: write`/`pull-requests: write`. Pass only `SOAK_AUTOFIX_TOKEN` to the soak module if it remains needed. Keep its `github.ref == 'refs/heads/main'` guard and force-with-lease behavior.

For `workflow_run`, execute routing and observer scripts from trusted default-branch `main`, never from `github.event.workflow_run.head_sha`, upstream artifacts, or a PR checkout. Preserve the existing eligibility checks for upstream event, branch and tag. The workflow-run trigger must watch only the category parents required by the monitor; do not include `Maintenance` itself. Otherwise observing a maintenance completion creates another maintenance completion and a recursive trigger chain.

Exit criterion: all original scheduled automation has one owner, manual default is offline validation, and no PR can select an issue writer, cleanup apply, or bot mutation caller.

## Phase 6 Update contracts and monitoring atomically

### Release gate and tag rider dispatches

Edit both simulator dispatch and polling loops in `release-packages.yml`. Replace the independent `simctl-tests.yml` run with `integration.yml -f suite=simctl-tests`; preserve candidate ref, exact head SHA, timeout, and API retry behavior.

The simulator success criterion becomes the actual nested job `simctl-tests / simctl` completing successfully on the candidate SHA. A green Integration run that only checked containers or app patterns must not qualify. An unrelated failure in another Integration suite must not erase an already successful simulator verdict. Likewise, a successful parent with simulator skipped must not qualify. Keep core release gating on exact `full-suite-gate` in `test.yml`, including the existing verified ancestor-reuse rules.

When deciding whether a simulator run already exists, inspect nested jobs/selection instead of using `.total_count` of all Integration runs on the SHA. A containers-only run must not suppress dispatch of simulator verification. Distinguish not selected, pending, succeeded and failed simulator results; a skipped/unselected job is not failure evidence or success evidence. Use pagination for job listings.

Replace the create-release tag-rider loop with three explicit best-effort dispatches:

```bash
gh workflow run documentation.yml --ref "$TAG" -R "$REPO" -f suite=docs
gh workflow run performance.yml --ref "$TAG" -R "$REPO" -f suite=benchmark
gh workflow run integration.yml --ref "$TAG" -R "$REPO" -f suite=container-tests
```

Retain the current warning-only failure treatment for these rider dispatches and the already-established ordering after stable publication. Keep publishing actions and OIDC inside `release-packages.yml` and `release-hono-server.yml`. Do not move them into a new caller file or change the npm Trusted Publisher identity, dist-tags, or credentials.

### Freshness source mapping

In `scripts/gate_freshness.json`, retain logical `workflow` values for the old suite identity and add/change `source_workflow` to the category parent. Add explicit `job_names` for the subject verdict; never use category run completion alone.

| Logical suite | Source parent | Required executed nested job names |
| --- | --- | --- |
| gc-ratchet | gc.yml | `gc-ratchet / gc-ratchet` |
| gc-root-dominance | gc.yml | `gc-root-dominance / gc-root-dominance`, `gc-root-dominance / gc-root-dominance-statepoints` |
| gc-native-roots | gc.yml | `gc-native-roots / gc-native-roots-complete` |
| gc-moving-witnesses | gc.yml | `gc-moving-witnesses / gc-moving-witnesses` |
| gc-parse-churn-gate | gc.yml | `gc-parse-churn-gate / gc-parse-churn-gate` |
| gc-ptr-shape-off-witness | gc.yml | `gc-ptr-shape-off-witness / gc-ptr-shape-off-witness` |
| auto-opt-app-patterns | integration.yml | `auto-opt-app-patterns / auto-opt-app-patterns` |
| eh-transport | compiler-runtime.yml | `eh-transport / eh-transport` |
| llvm-inprocess | compiler-runtime.yml | `llvm-inprocess / llvm-inprocess-complete` |
| tls-budget | performance.yml | `tls-budget / tls-budget` |
| npm-publish-freshness | maintenance.yml | `npm-publish-freshness / npm-publish-freshness` |
| security-audit | test.yml | `security-audit / security-audit`, `security-audit / soak-gate`, `security-audit / config-scans` |

Retain each existing age budget. Use strict native-roots fan-in to cover its full matrix. Keep failed completed subject jobs as fresh execution evidence; failures and staleness are different signals. Missing/skipped/cancelled subject jobs are not execution evidence. Confirm actual rendered API names during rollout before considering this migration verified; do not guess matrix suffixes in freshness lists.

Paginate jobs APIs in freshness and observer code. Consolidated runs can exceed the existing `per_page=100` sample. A required job on page two must not be reported missing, and an upstream failure on page two must not disappear.

### Failure observer mapping

Update `scripts/gate_failure_watch.json` and `scripts/gate_failure_watch.py` together:

- Watch parent paths/names `test.yml`/`CI`, `gc.yml`/`GC`, `compiler-runtime.yml`/`Compiler and Runtime`, `integration.yml`/`Integration`, and `performance.yml`/`Performance` for the existing monitored subjects.
- Carry logical suite-to-parent and nested subject selectors, so observing one scheduled suite does not mark an unselected sibling as passing or failing. Retain sticky issue identities based on logical suite paths where practical, avoiding duplicate issues for the same existing failure.
- Process actual subject jobs: green subject closes its issue, red subject updates it, absent/skipped subject leaves its issue alone. Preserve all failure rows/step names for the selected logical subject.
- Treat the core CI subject separately from new auxiliary CI schedules: weekly coverage must not close an existing core CI failure. Require a successful/failed actual core gate when evaluating the CI subject.
- Maintenance's freshness checker and npm registry checker already maintain their own alerts. Represent these logical subjects explicitly as self-alerting exclusions and keep Maintenance out of its own workflow-run subscription.
- Change `WATCH_WORKFLOW` from the old child to `.github/workflows/maintenance.yml` for subscription/config validation. Validate parent names, logical subject selectors and self-alerting exclusions; remove the old assumption that each tracked logical workflow has its own standalone event run.
- On configuration changes, unknown/empty subjects must fail validation rather than silently dropping monitored gates.

Use synthetic historical API rows to prove sticky issue updates/closures are tied to the correct subject. During local validation all observer tests use mock requests or `--dry-run`; do not create, close, or comment on live issues.

### Static gate wiring

Update `scripts/gc_gate_wiring_check.py` to trace the explicit caller graph. The existing registered GATES still identify child file/job IDs; a call-only child is main-line reachable when a parent schedule selects that child and its job conditions permit execution.

Do not weaken the checker by treating every `workflow_call` file as reachable. It must fail when its caller disappears, its cron disappears, its router rejects its schedule, the child subject is always skipped, or `continue-on-error` masks the gate. Preserve concurrency checks for parents and children and all existing sabotage self-tests.

Keep the three Node-version exemption filenames unchanged because child filenames and publisher files remain unchanged. Run the consistency audit; only amend an exemption if a real file/value contract changed. Do not change `.node-version` or published performance baselines.

Search runtime dependencies with `rg` across `.github/workflows`, `scripts`, `docs/src/testing`, and root package scripts for old dispatch targets. Update current operational examples and references to parent dispatches; historical changelog/audit records can retain original names. Do not replace every occurrence of a child filename: child implementations and exemption references remain valid.

### Regression hooks

Add the new routing/topology validations to CI lint using the existing Python dependency setup. Treat `scripts/actions_catalog.json`, `scripts/actions_plan.py`, topology checks, routing tests and affected CI modules as CI configuration changes in `scripts/ci_plan.py` without turning every unrelated category implementation into an expensive core full-tier change.

Retain the existing `ci_plan.py` self-test asserting that unrelated benchmark workflow changes are not core, or replace it with an equally meaningful unrelated-category example if its status truly changes. Do not delete assertions to pass migration tests.

## Phase 7 Required validation and acceptance cases

Run only checks relevant to this workflow refactor. No compiler build, parity sweep, performance measurement, package publication or full Rust test suite is needed for local completion.

Prepare an isolated temporary Python environment with PyYAML if the current Python lacks it; do not install into system Python. Use the repo's pinned Node version for Node-based checks. Use a current documented actionlint installation for syntax validation and record its version; do not edit the repository's external tool pin registry merely to run local actionlint.

Run these existing checks after the migration:

```text
python scripts/ci_plan.py --self-test
python scripts/gc_gate_wiring_check.py --self-test
python scripts/gc_gate_wiring_check.py
python scripts/check_gate_freshness.py --self-test
python scripts/gate_failure_watch.py --self-test
python scripts/gate_failure_watch.py --check-config
python scripts/check_node_version_consistency.py --self-test
python scripts/check_node_version_consistency.py --list
python scripts/check_npm_publish_freshness.py --self-test
python scripts/check_npm_publish_freshness.py --check-manifest
python scripts/reap_stale_ci_runs.py --self-test
python scripts/test_actions_plan.py
python scripts/check_actions_topology.py
git diff --check
```

Run actionlint on changed callers, changed callees and release workflows. Run the repository's existing pinned zizmor workflow command against `.github` without a token when one is unnecessary; do not use a live token merely to validate local changes. Distinguish unrelated baseline findings from new findings; no blanket ignores or severity reduction.

Add focused offline regression coverage for these acceptance cases:

1. Exactly the 10 listed entrypoints have independent triggers, every mapped source is owned once, every caller exists, and no call graph cycle exists.
2. Every catalog cron is present on its parent, no extra unknown cron is accepted, and each original cron selects exactly its original suite(s).
3. GC's six distinct cron minutes do not launch six GC suites per cron. GC manual `all` selects all six in one parent run.
4. Shared Integration cron `17 3 * * *` selects Next.js only; Compatibility's same cron selects Node core only, within its own parent.
5. Unlabelled PR heavy gates remain skipped; `run-extended-tests` and `run-cc-parity` retain their separate meanings.
6. Docs-only PR still has core CI `pr-gate`; routing failure cannot cause a skipped required PR gate.
7. Auxiliary CI coverage/security schedules do not run core CI and cannot emit `full-suite-gate` even when dispatch input `tier=full` is present.
8. Core full dispatch still produces an unprefixed `full-suite-gate`; all existing fan-in dependencies remain accounted for.
9. Normal Node core/package sweeps are not newly enabled on PRs; merge-group selects only the two original Node guard suites.
10. PR and main push path matching reproduces original filters, including root/nested paths, zero-directory `**/`, and the three container type directories.
11. `limit=0`, `max_per_api='0'`, `strict=false`, and both false container options survive typed forwarding.
12. Compatibility manual suite selection does not run unrelated suite jobs. Manual `all` forwards defaults to each module.
13. Documentation validation default does not select deployment; `suite=docs` does, and has deployment permissions only on that caller.
14. A `v*` tag selects only original tag riders. A `hono-server-v*` tag selects Hono publication only. Performance `release: published` selects benchmark only.
15. Maintenance manual default validates offline, no issue writes/queue apply/bot mutation. Topic-branch live maintenance dispatch fails clearly.
16. Maintenance PR execution is read-only, while scheduled runtime callers have only their necessary permissions.
17. An upstream workflow-run from a PR/ref cannot substitute its code for trusted main observer code. Maintenance is absent from its own subscription.
18. Release on SHA A does not accept successful simulator on SHA B, an Integration run without simulator, or skipped simulator. A containers-only run on SHA A does not suppress simulator dispatch.
19. Simulator success in an Integration run with an unrelated suite failure is still recognized by the simulator subject criterion. Pending simulator continues polling; completed failed simulator produces a failure signal.
20. Release core gate never accepts `main-gate` or an auxiliary CI green in place of `full-suite-gate`.
21. Parent green with a skipped monitored suite does not refresh or close that suite's issue; sibling failures do not overwrite its verdict.
22. Completed red subject is fresh execution evidence but updates its failure issue; cancelled/missing subject is not execution evidence.
23. Freshness requires both root-dominance jobs and the native-roots strict fan-in. Security freshness requires the actual `config-scans` job instead of obsolete removed jobs.
24. Required jobs and failed subjects appearing on job API page two are found correctly.
25. Observer closes the existing subject issue on subject success, leaves it unchanged when unselected, and never calls a live issue API during self-tests.
26. Deleting a GC caller/schedule or making its subject permanently skipped fails the wiring audit.
27. Caller/child concurrency groups differ, main-line scheduled runs use unique run IDs, and only PR events supersede themselves.
28. Release matrix is an object with its original include targets, contains no misplaced action invocation, and publisher identities/inputs are unchanged.

Review the final diff using the inventory. For every source list intentional changes to event ownership, typed forwarding, concurrency corrections and monitor contracts. Any change to build scripts, runtime source, oracle versions, artifact names, package publication semantics or unrelated test thresholds is outside this task and must be removed or escalated with evidence.

Add a changeset describing syntax repair, categories, call-only suites, contract migrations and validation. Follow the repository's version policy at landing; if acting as an external contributor, do not preemptively bump workspace metadata. If maintainer preparation requires a patch bump, update Cargo.toml/Cargo.lock and only CLAUDE.md's Current Version line together. Do not append prose to CLAUDE.md or CHANGELOG.md.

Exit criterion: local relevant checks pass, only planned files are changed, no independent child triggers remain, and the review notes clearly distinguish local validation from GitHub rollout evidence.

## Phase 8 Operator rollout after review

This phase needs the operator's release/merge intent; it is not part of merely writing or locally implementing this plan.

1. Review the syntax repair separately if practical, then the atomic consolidation change. Do not deploy half a category with two simultaneous trigger owners.
2. Let required PR CI validate the migration. Inspect fork/PR permission behavior and the core gate names.
3. After landing, run small category selections to verify nested names and routing. Use read-only selections first. Do not dispatch a Release workflow just to inspect naming; it can publish. Do not choose Maintenance live tasks merely to inspect naming.
4. Verify one scheduled execution for each monitored logical subject, and use real nested API names to confirm the freshness manifest. Do not turn a missing scheduled run into an artificially green freshness baseline.
5. Confirm release polling using offline fixtures and an operator-authorized build/stage verification path before a stable publication; stage mode alone is not proof that stable release gates execute.
6. Inventory the seven orphan diagnostic registrations and old standalone registrations. Call-only converted workflows may retain historic sidebar entries. GitHub has no custom nested sidebar folders, and disabling old registrations does not guarantee their history disappears from the UI.
7. Do not blindly disable converted child workflow IDs: disabling a file may interfere with its reuse. Prefer stopping independent triggers in YAML and verifying callable behavior. Only disable confirmed orphan workflows that have no current file/caller after operator review.
8. Preserve historical logs and artifacts. If the operator wants a completely clean sidebar and GitHub requires removing old histories, prepare the exact workflow IDs, run counts, artifacts and consequences for a separate reviewed deletion decision. Never promise that 10 event entrypoints guarantees only 10 visible historical registrations.
9. Keep GitHub-managed entries as platform entries. Update `docs/src/testing/ci-tiers.md` and `ci-gate-scheduling.md` with the live category controls and manual dispatch examples.

If rollout exposes a category problem, restore that whole category's prior trigger ownership and its dependent monitor/dispatch configuration together. Do not leave both parent and child event triggers enabled. Retain the independent release syntax repair when reverting consolidation.

## Stop conditions for the implementation agent

Stop the affected phase and report the exact evidence when:

- Refreshed main changes a required gate or publisher identity that this plan depends on.
- A reusable caller needs a writable PR token, unknown secret, or new npm publisher identity to work.
- A release query cannot distinguish selected simulator/core jobs from unrelated successes.
- A check passes only after deleting a liveness assertion, weakening the gate audit, adding continue-on-error, or accepting skipped jobs as execution.
- The plan's nested job name assumptions differ from actual API names; update verified selectors instead of weakening freshness.
- Complete sidebar cleanup would require deleting logs/artifacts or disabling a currently called workflow.

Routine implementation failures should be fixed and checks rerun; they do not require a user permission request. No subagents are required. Keep changes in the current worktree and give a concise handoff of completed phases, validation, and remaining operational verification.

## Prompt for Luna

```text
Use gpt-6-luna, low reasoning, with Fast enabled in the client.

Implement docs/audits/actions-consolidation-plan.md through Phase 7 in order.
Read CLAUDE.md fully first. Use the companion inventory for source contracts,
refresh against latest origin/main, and preserve unrelated work. Follow the
specified topology, filenames, typed input mappings, schedule routing, CI gate
exceptions, maintenance trust boundaries, and release/monitor migrations.

Run the relevant offline checks and required regression cases. Do not run Rust
builds or full compatibility/performance sweeps for this YAML/Python refactor.
Do not execute Phase 8, publish, merge, cancel runs, modify live issues, or delete
run history. Do not weaken gates or invent new publisher identities. Report a
stop condition with concrete evidence; otherwise fix routine failures and
continue. Finish with reviewable changes, validation results, and the remaining
GitHub rollout checks. Do not spawn other agents.
```

GitHub reference: <https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows> and <https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax>.
