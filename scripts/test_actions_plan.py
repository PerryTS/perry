"""Focused offline checks for Actions category routing and path matching."""

from __future__ import annotations

import json
import unittest

from actions_plan import load_catalog, path_matches, select


CATALOG = load_catalog()


def selected(category: str, event_name: str, event: dict, ref: str = "refs/heads/main",
             paths: list[str] | None = None, suite: str | None = None) -> list[str]:
    result = select(category, event_name, event, ref, paths, suite, CATALOG)
    return [key for key, value in json.loads(result["plan"]).items() if key != "module_inputs" and value]


class PathPatternTests(unittest.TestCase):
    def test_single_star_does_not_cross_directory_boundary(self):
        self.assertTrue(path_matches(["npm/*.json"], ["npm/manifest.json"]))
        self.assertFalse(path_matches(["npm/*.json"], ["npm/dir/manifest.json"]))

    def test_double_star_matches_zero_or_more_directories(self):
        self.assertTrue(path_matches(["**/Cargo.toml"], ["Cargo.toml"]))
        self.assertTrue(path_matches(["**/Cargo.toml"], ["crates/perry/Cargo.toml"]))

    def test_ordered_exclusions_can_be_reincluded(self):
        patterns = ["docs/**", "!docs/generated/**", "docs/generated/README.md"]
        self.assertTrue(path_matches(patterns, ["docs/index.md"]))
        self.assertFalse(path_matches(patterns, ["docs/generated/page.md"]))
        self.assertTrue(path_matches(patterns, ["docs/generated/README.md"]))


class CategoryRoutingTests(unittest.TestCase):
    def test_workflow_contract_has_38_owned_modules_and_six_parents(self):
        categories = CATALOG["categories"]
        self.assertEqual(len(categories), 10)
        self.assertEqual(len({category['entrypoint'] for category in categories.values()}), 6)
        files = [module["file"] for c in categories.values() for module in c["modules"]]
        self.assertEqual(len(files), 38)
        self.assertEqual(len(set(files)), 38)

    def test_each_gc_cron_selects_only_its_suite(self):
        for module in CATALOG["categories"]["gc"]["modules"]:
            for trigger in module["original_events"].get("schedule", []):
                with self.subTest(module=module["id"], cron=trigger["cron"]):
                    self.assertEqual(selected("gc", "schedule", {"schedule": trigger["cron"]}),
                                     [module["id"]])

    def test_shared_cron_is_scoped_to_category(self):
        cron = "17 3 * * *"
        self.assertEqual(selected("integration", "schedule", {"schedule": cron}), ["next-app-route"])
        self.assertEqual(selected("compatibility", "schedule", {"schedule": cron}), ["node-core-subset"])

    def test_each_catalog_cron_is_parented_and_selects_its_owner(self):
        for key, category in CATALOG["categories"].items():
            expected: dict[str, list[str]] = {}
            for module in category["modules"]:
                for entry in module.get("original_events", {}).get("schedule", []):
                    expected.setdefault(entry["cron"], []).append(module["id"])
            for cron, modules in expected.items():
                with self.subTest(category=key, cron=cron):
                    self.assertEqual(selected(key, "schedule", {"schedule": cron}), modules)

    def test_gc_dispatch_all_selects_all_six_suites(self):
        actual = selected("gc", "workflow_dispatch", {}, suite="all")
        self.assertEqual(len(actual), 6)
        self.assertEqual(len(selected("gc","workflow_dispatch",{},suite="all")),6)

    def test_unknown_schedule_fails_closed(self):
        with self.assertRaisesRegex(ValueError, "unrecognized gc schedule"):
            selected("gc", "schedule", {"schedule": "1 1 * * *"})

    def test_extended_suites_are_labelled_and_not_unlabelled_by_router(self):
        event = {"action": "synchronize", "pull_request": {"base": {"ref": "main"}}}
        selected_ids = selected("gc", "pull_request", event, paths=["crates/perry-runtime/src/gc/mod.rs"])
        self.assertEqual(selected_ids, [])
        labels = {module["id"]: module.get("pr_label") for module in CATALOG["categories"]["gc"]["modules"]}
        self.assertTrue(all(label == "run-extended-tests" for label in labels.values()))
        event["pull_request"]["labels"]=[{"name":"run-extended-tests"}]
        selected_ids=selected("gc","pull_request",event,paths=["crates/perry-runtime/src/gc/mod.rs"])
        self.assertIn("gc-ratchet",selected_ids)
        self.assertEqual(selected("gc","pull_request",{"action":"labeled","label":{"name":"skip-changelog"},"pull_request":{"base":{"ref":"main"},"labels":[{"name":"run-extended-tests"}]}},paths=["crates/perry-runtime/src/gc/mod.rs"]),[])

    def test_cc_parity_uses_its_separate_label(self):
        event={"action":"synchronize","pull_request":{"base":{"ref":"main"},"labels":[]}}
        paths=["crates/perry-codegen/src/lib.rs"]
        self.assertNotIn("cc-parity",selected("compiler-runtime","pull_request",event,paths=paths))
        event["pull_request"]["labels"]=[{"name":"run-cc-parity"}]
        self.assertIn("cc-parity",selected("compiler-runtime","pull_request",event,paths=paths))

    def test_ci_docs_pr_still_requires_core_gate(self):
        event = {"action": "opened", "pull_request": {"base": {"ref": "main"}}}
        result = select("ci", "pull_request", event, "refs/pull/7/merge", ["docs/src/index.md"], catalog=CATALOG)
        self.assertEqual(result["run_core"], "true")
        self.assertIn("core", json.loads(result["plan"]))

    def test_ci_v_tag_keeps_core_full_run_and_no_auxiliaries(self):
        event={"ref":"refs/tags/v0.5.1657"}
        result=select("ci","push",event,"refs/tags/v0.5.1657",catalog=CATALOG)
        self.assertEqual(result['run_core'],'true')
        self.assertEqual([k for k, v in json.loads(result['plan']).items() if v], ["core"])

    def test_core_and_auxiliary_ci_parent_jobs_preserve_gate_shape(self):
        from pathlib import Path
        import yaml

        workflow = yaml.load(Path(".github/workflows/test.yml").read_text(), Loader=yaml.BaseLoader)
        jobs = workflow["jobs"]
        self.assertIn("gate", jobs)
        self.assertIn("pr-gate", jobs["gate"]["name"])
        self.assertIn("full-suite-gate", jobs["gate"]["name"])
        self.assertNotIn("core-pr-gate", jobs["gate"]["name"])
        full = jobs["gate"]
        self.assertIn("plan", full.get("needs", []))
        self.assertIn("route", full.get("needs", []))
        self.assertIn("always()", full["if"])
        self.assertIn("github.event_name == 'pull_request'", full["if"])
        self.assertIn("route_result", full["steps"][0]["run"])
        self.assertTrue({"lint", "check", "warnings", "cargo-test", "security-audit"}.issubset(set(full["needs"])))
        self.assertEqual(jobs["security-weekly"]["name"], "Weekly Security Audit / suite result")
        self.assertIn("needs.route.outputs.plan", jobs["security-weekly"]["if"])
        for module, display_name in {
            "coverage": "Coverage / suite result",
            "zizmor": "zizmor / suite result",
            "native-result-ledger": "Native Result Ledger / suite result",
            "npm-launcher": "npm launcher / suite result",
        }.items():
            self.assertEqual(jobs[module]["name"], display_name)

    def test_auxiliary_ci_dispatch_does_not_run_core(self):
        result = select("ci", "workflow_dispatch", {"inputs":{"tier":"full"}}, suite="coverage", catalog=CATALOG)
        self.assertEqual(result["run_core"], "false")
        self.assertEqual([k for k, v in json.loads(result["plan"]).items() if k != "module_inputs" and v], ["coverage"])

    def test_weekly_security_is_auxiliary_but_all_includes_core(self):
        weekly = select("ci", "workflow_dispatch", {}, suite="security-audit", catalog=CATALOG)
        self.assertEqual(weekly["run_core"], "false")
        all_run = select("ci", "workflow_dispatch", {}, suite="all", catalog=CATALOG)
        self.assertEqual(all_run["run_core"], "true")

    def test_node_merge_group_runs_only_node_guards(self):
        actual = selected("compatibility", "merge_group", {})
        self.assertEqual(set(actual), {"node-compat-matrix", "node-suite-guard"})

    def test_zero_and_false_dispatch_values_are_preserved_by_typed_inputs(self):
        catalog = CATALOG["categories"]
        self.assertEqual(catalog["compatibility"]["dispatch_inputs"]["limit"]["default"], 6)
        self.assertEqual(catalog["compatibility"]["dispatch_inputs"]["strict"]["default"], False)
        self.assertEqual(catalog["integration"]["dispatch_inputs"]["run_e2e"]["default"], "false")
        self.assertEqual(catalog["integration"]["dispatch_inputs"]["run_fuzz"]["default"], "false")
        inputs = select("compatibility", "workflow_dispatch", {}, suite="npm-package-sweep", catalog=CATALOG)
        self.assertEqual(json.loads(inputs["module_inputs"])["npm-package-sweep"],
                         {"packages": "", "limit": 6, "strict": False})
        explicit = select("compatibility", "workflow_dispatch", {"inputs":{"limit":0,"strict":False}}, suite="npm-package-sweep", catalog=CATALOG)
        self.assertEqual(json.loads(explicit["module_inputs"])["npm-package-sweep"]["limit"], 0)
        node_core = select("compatibility", "workflow_dispatch", {"inputs":{"max_per_api":"0"}}, suite="node-core-subset", catalog=CATALOG)
        self.assertEqual(json.loads(node_core["module_inputs"])["node-core-subset"]["max_per_api"], "0")

    def test_documentation_default_does_not_deploy(self):
        self.assertEqual(selected('documentation', 'workflow_dispatch', {}), [])
        self.assertEqual(selected("documentation", "workflow_dispatch", {}, suite="docs-check"), ["docs-check"])
        self.assertEqual(selected("documentation", "workflow_dispatch", {}, suite="docs"), ["docs"])
        with self.assertRaises(ValueError):
            selected('documentation', 'workflow_dispatch', {}, suite='all')

    def test_shared_parent_dispatch_selects_only_requested_suite_or_category(self):
        keys = CATALOG['entrypoints']['compiler-runtime.yml']['categories']
        for owner in keys:
            owned = [module['id'] for module in CATALOG['categories'][owner]['modules']]
            for requested in [owner, *owned]:
                for category in keys:
                    with self.subTest(requested=requested, category=category):
                        expected = owned if requested == owner else [requested]
                        self.assertEqual(selected(category, 'workflow_dispatch', {}, suite=requested),
                                         expected if category == owner else [])
        for category in keys:
            self.assertEqual(selected(category, 'workflow_dispatch', {}, suite='all'),
                             [module['id'] for module in CATALOG['categories'][category]['modules']])

    def test_shared_parent_schedules_allow_inactive_sibling_routes(self):
        for parent in CATALOG['entrypoints'].values():
            for owner in parent['categories']:
                for module in CATALOG['categories'][owner]['modules']:
                    for row in module['original_events'].get('schedule', []):
                        for category in parent['categories']:
                            expected = [m['id'] for m in CATALOG['categories'][category]['modules']
                                        if row in m['original_events'].get('schedule', [])]
                            with self.subTest(category=category, cron=row['cron']):
                                self.assertEqual(selected(category, 'schedule', {'schedule':row['cron']}), expected)

    def test_repository_dispatch_preserves_live_task_boundaries(self):
        for suite in ('validate', 'delete-retired-runs', 'gate-freshness'):
            self.assertEqual(selected('documentation', 'workflow_dispatch', {}, suite=suite), [])
        for suite in ('docs-check', 'docs'):
            result = select('maintenance', 'workflow_dispatch', {}, ref='refs/tags/v1.2.3', suite=suite)
            self.assertEqual(result['selection'], '')
            self.assertEqual(result['validate'], 'false')

    def test_documentation_push_does_not_start_retired_run_cleanup(self):
        for paths, want in [(['docs/src/index.md'], 'false'),
                            (['.github/workflows/maintenance.yml'], 'true'),
                            (['scripts/delete_retired_workflow_runs.py'], 'true')]:
            result = select('maintenance', 'push', {'ref':'refs/heads/main'}, changed_paths=paths)
            self.assertEqual(result['cleanup'], want)
        self.assertEqual(select('maintenance', 'push', {'ref':'refs/tags/v1.2.3'},
                                ref='refs/tags/v1.2.3')['cleanup'], 'false')

    def test_hermetic_container_step_waits_for_every_suite_and_reports_failure(self):
        import os
        import subprocess
        import tempfile
        from pathlib import Path
        import yaml

        workflow = yaml.load(Path('.github/workflows/compiler-runtime.yml').read_text(), Loader=yaml.BaseLoader)
        steps = workflow['jobs']['container-tests__hermetic']['steps']
        script = next(step['run'] for step in steps
                      if step.get('name') == 'Run and await all hermetic container suites')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cargo = root / 'cargo'
            cargo.write_text('#!/bin/bash\nprintf "cargo %s\\n" "$*"\n'
                             'case "$*" in *container_extra_tests*) exit "${STUB_FFI_EXIT:-0}" ;; esac\n')
            cargo.chmod(0o755)
            for fail in ('0', '1'):
                result = subprocess.run(['bash', '-c', script], text=True, capture_output=True,
                                        env={**os.environ, 'PATH': str(root) + os.pathsep + os.defpath,
                                             'RUNNER_TEMP': str(root), 'STUB_FFI_EXIT': fail}, timeout=10)
                self.assertEqual(result.returncode, int(fail), result.stderr)
                self.assertEqual(result.stdout.count('cargo test '), 6)
                self.assertEqual(result.stdout.count('::endgroup::'), 6)
                self.assertEqual('::error::ffi-regressions failed' in result.stdout, fail == '1')

    def test_manual_all_forwards_original_module_defaults(self):
        compat=select("compatibility","workflow_dispatch",{},suite="all",catalog=CATALOG)
        values=json.loads(compat["module_inputs"])
        self.assertEqual(values["node-core-subset"],{"apis":"","max_per_api":"25"})
        self.assertEqual(values["npm-package-sweep"],{"packages":"","limit":6,"strict":False})
        integration=select("integration","workflow_dispatch",{},suite="all",catalog=CATALOG)
        iv=json.loads(integration["module_inputs"])
        self.assertEqual(iv["container-tests"],{"run_e2e":"false","run_fuzz":"false"})

    def test_release_tag_routes_select_only_original_category_subjects(self):
        event = {"ref": "refs/tags/v0.5.1657"}
        actual = selected("performance", "push", event, "refs/tags/v0.5.1657")
        self.assertEqual(set(actual), {"benchmark", "tak-performance", "tls-budget"})
        self.assertEqual(selected("performance", "release", {"action": "published"}), ["benchmark"])
        # An explicitly selected benchmark rider does not implicitly start
        # siblings; direct v* tags still preserve each source workflow's rule.
        dispatched = select("performance", "workflow_dispatch", {},
                            suite="benchmark", catalog=CATALOG)
        self.assertEqual([k for k, v in json.loads(dispatched["plan"]).items() if k != "module_inputs" and v], ["benchmark"])
        hono = selected("release-hono-server", "push", {"ref": "refs/tags/hono-server-v1.2.3"},
                        "refs/tags/hono-server-v1.2.3")
        self.assertEqual(hono, ["release-hono-server"])

    def test_pr_paths_route_only_relevant_modules(self):
        event = {"action": "opened", "pull_request": {"base": {"ref": "main"}}}
        self.assertEqual(selected("documentation", "pull_request", event, paths=["README.md"]), ["docs-check"])
        for path in ("types/perry/container/index.ts", "types/perry/compose/index.ts",
                     "types/perry/workloads/index.ts"):
            with self.subTest(path=path):
                self.assertEqual(set(selected("integration", "pull_request", event, paths=[path])),
                                 {"container-tests"})
                labelled={"action":"opened","pull_request":{"base":{"ref":"main"},"labels":[{"name":"run-extended-tests"}]}}
                self.assertEqual(set(selected("integration","pull_request",labelled,paths=[path])),
                                 {"container-tests","auto-opt-app-patterns"})

    def test_maintenance_dispatch_defaults_to_offline_validation(self):
        result = select("maintenance", "workflow_dispatch", {}, suite="validate", catalog=CATALOG)
        self.assertEqual(result["selection"], "")
        self.assertEqual(result["run_core"], "false")
        self.assertTrue(CATALOG["categories"]["maintenance"]["dispatch_inputs"]["apply"]["default"] is False)
        with self.assertRaisesRegex(ValueError, "unknown Maintenance"):
            select("maintenance", "workflow_dispatch", {}, suite="all", catalog=CATALOG)

    def test_retired_run_cleanup_dispatch_selects_no_unrelated_suite(self):
        result = select("maintenance", "workflow_dispatch", {}, suite="delete-retired-runs", catalog=CATALOG)
        self.assertEqual(result["selection"], "")
        self.assertFalse(any(json.loads(result["plan"]).values()))
        with self.assertRaisesRegex(ValueError, "only from main"):
            select("maintenance", "workflow_dispatch", {}, ref="refs/heads/topic",
                   suite="delete-retired-runs", catalog=CATALOG)

    def test_maintenance_workflow_run_accepts_only_trusted_upstream_runs(self):
        def event(name, branch="main", trigger="schedule"):
            return {"workflow_run":{"name":name,"head_branch":branch,"event":trigger}}
        self.assertEqual(selected("maintenance","workflow_run",event("GC")),["gate-failure-watch"])
        self.assertEqual(selected("maintenance","workflow_run",event("Extended Tests")),["gate-failure-watch"])
        self.assertEqual(selected("maintenance","workflow_run",event("GC","fork","pull_request")),[])
        self.assertEqual(selected("maintenance","workflow_run",event("GC","topic","workflow_dispatch")),[])
        self.assertEqual(selected("maintenance","workflow_run",event("Maintenance")),[])
        self.assertEqual(selected("maintenance","workflow_run",event("Repository")),[])

    def test_monitor_subject_job_contracts_include_strict_fanins(self):
        by_file={m['file']:m for c in CATALOG['categories'].values() for m in c['modules']}
        self.assertEqual(set(by_file['gc-root-dominance.yml']['required_jobs']),{'gc-root-dominance','gc-root-dominance-statepoints'})
        self.assertEqual(by_file['gc-native-roots.yml']['required_jobs'],['gc-native-roots-complete'])
        self.assertIn('config-scans',by_file['security-audit.yml']['required_jobs'])

    def test_push_payload_paths_are_complete_or_fail_open(self):
        from actions_plan import _push_paths
        self.assertEqual(_push_paths({"size": 1, "commits": [{"added": [".github/x.yml"]}]}), [".github/x.yml"])
        self.assertIsNone(_push_paths({"size": 2, "commits": [{"modified": ["x"]}]}))


if __name__ == "__main__":
    unittest.main()
