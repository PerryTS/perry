#!/usr/bin/env python3
"""Check Actions entrypoints, inline suites, routing catalog and trigger unions."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / '.github/workflows'
CATALOG = json.loads((ROOT / 'scripts/actions_catalog.json').read_text())
EXPECTED = {
    'test.yml', 'gc.yml', 'compiler-runtime.yml',
    'release-packages.yml', 'release-hono-server.yml', 'maintenance.yml',
}


def read(path: Path):
    return yaml.load(path.read_text(), Loader=yaml.BaseLoader) or {}


def trigger_set(workflow):
    triggers = workflow.get('on', workflow.get(True, {}))
    if isinstance(triggers, list):
        return set(triggers)
    return set(triggers) if isinstance(triggers, dict) else set()


def main() -> int:
    errors = []
    owners = {}
    workflows = {path.name: read(path) for path in WORKFLOWS.glob('*.yml')}
    for name, workflow in workflows.items():
        for job_id, job in (workflow.get('jobs') or {}).items():
            display_name = str(job.get('name') or job_id)
            # Expression identifiers such as fromJSON are case-sensitive.
            literal = re.sub(r'\$\{\{.*?\}\}', '', display_name)
            if literal != literal.lower():
                errors.append(f'{name}/{job_id}: job display name must be lowercase')
            if '__' in job_id:
                suite_id = job_id.split('__', 1)[0]
                if not display_name.startswith(f'{suite_id} / '):
                    errors.append(f'{name}/{job_id}: display name must use its stable suite ID')
            elif display_name.endswith('/ suite result'):
                if display_name != f'{job_id} / suite result':
                    errors.append(f'{name}/{job_id}: result job must use its stable suite ID')
    active = {
        name for name, workflow in workflows.items()
        if any(event != 'workflow_call' for event in trigger_set(workflow))
    }
    if active != EXPECTED:
        errors.append(
            f'entrypoints mismatch: missing={sorted(EXPECTED - active)} '
            f'extra={sorted(active - EXPECTED)}'
        )

    for category_id, category in CATALOG['categories'].items():
        parent = category['entrypoint']
        if parent not in workflows:
            errors.append(f'{category_id}: missing parent {parent}')
            continue
        workflow = workflows[parent]
        triggers = trigger_set(workflow)
        jobs = workflow.get('jobs') or {}
        route_job = category.get('route_job', 'route')
        if category.get('route_job') and route_job not in jobs:
            errors.append(f'{parent}/{category_id}: missing category router {route_job}')
        concurrency = workflow.get('concurrency') or {}
        parent_group = str(concurrency.get('group') or '')
        if category_id not in {'ci', 'release-packages', 'release-hono-server'}:
            if 'github.run_id' not in parent_group:
                errors.append(f'{parent}: non-PR concurrency must use github.run_id')
            if "github.event_name == 'pull_request'" not in str(concurrency.get('cancel-in-progress') or ''):
                errors.append(f'{parent}: only PR runs may supersede one another')

        for module in category['modules']:
            source = module['file']
            module_id = module['id']
            if source in owners:
                errors.append(f'{source}: owned by both {owners[source]} and {category_id}')
            owners[source] = category_id
            if source == parent:
                continue
            if source in workflows:
                errors.append(f'{source}: retired reusable workflow still exists')

            summary = jobs.get(module_id)
            if not summary:
                errors.append(f'{parent}: missing suite result job {module_id}')
                continue
            suite_jobs = {
                job_id: job for job_id, job in jobs.items()
                if job_id.startswith(f'{module_id}__')
            }
            if not suite_jobs:
                errors.append(f'{parent}/{module_id}: missing inlined suite jobs')
            if not set(suite_jobs).issubset(set(summary.get('needs') or [])):
                errors.append(f'{parent}/{module_id}: result job does not depend on every suite job')
            if 'always()' not in str(summary.get('if') or ''):
                errors.append(f'{parent}/{module_id}: result job must run after failed or skipped jobs')
            if summary.get('name') != f'{module_id} / suite result':
                errors.append(f'{parent}/{module_id}: result job must use its stable suite ID')
            if category.get('route_job') and f'needs.{route_job}.outputs.plan' not in str(summary.get('if') or ''):
                errors.append(f'{parent}/{module_id}: result job must use its category router')

            for job_id, job in jobs.items():
                if isinstance(job, dict) and str(job.get('uses') or '').startswith('./.github/workflows/'):
                    errors.append(f'{parent}/{job_id}: nested reusable workflow call remains')

            for required in module.get('required_jobs', []):
                if not any(
                    required in job_id or required in str((job or {}).get('name') or '')
                    for job_id, job in suite_jobs.items()
                ):
                    errors.append(f'{parent}/{module_id}: required suite job {required} missing')

            module_schedules = {
                row['cron'] for row in module.get('original_events', {}).get('schedule', [])
            }
            if module_schedules and 'schedule' not in triggers:
                errors.append(f'{parent}: missing schedule trigger(s) {sorted(module_schedules)}')

        actual_schedules = {
            row.get('cron') for row in (workflow.get('on') or {}).get('schedule', [])
            if isinstance(row, dict)
        }
        expected_schedules = {
            row['cron']
            for sibling in CATALOG['categories'].values()
            if sibling['entrypoint'] == parent
            for module in sibling['modules']
            for row in module.get('original_events', {}).get('schedule', [])
        }
        if actual_schedules != expected_schedules:
            errors.append(
                f'{parent}: schedule mismatch missing={sorted(expected_schedules - actual_schedules)} '
                f'extra={sorted(actual_schedules - expected_schedules)}'
            )

    for parent, spec in CATALOG.get('entrypoints', {}).items():
        workflow = workflows.get(parent) or {}
        inputs = (workflow.get('on') or {}).get('workflow_dispatch', {}).get('inputs', {})
        expected_inputs = yaml.load(yaml.safe_dump(spec['dispatch_inputs']), Loader=yaml.BaseLoader)
        if inputs != expected_inputs:
            errors.append(f'{parent}: manual inputs differ from parent routing contract')
        if workflow.get('name') != spec['name']:
            errors.append(f'{parent}: display name differs from parent routing contract')
        if set(spec['categories']) != {
            key for key, category in CATALOG['categories'].items() if category['entrypoint'] == parent
        }:
            errors.append(f'{parent}: parent category membership differs from routing contract')

    freshness = json.loads((ROOT / 'scripts/gate_freshness.json').read_text())
    for gate in freshness['gates']:
        parent = gate.get('source_workflow', gate['workflow'])
        names = {
            job.get('name') or job_id
            for job_id, job in (workflows.get(parent, {}).get('jobs') or {}).items()
        }
        for name in gate.get('job_names', []):
            if name not in names:
                errors.append(f'{parent}: freshness selector does not match a job: {name}')

    release = workflows.get('release-packages.yml') or {}
    cross = ((release.get('jobs') or {}).get('build-cross') or {})
    matrix = ((cross.get('strategy') or {}).get('matrix') or {})
    rows = matrix.get('include') or []
    if set(matrix) != {'include'} or not isinstance(rows, list) or len(rows) != 8:
        errors.append('release-packages.yml/build-cross: expected matrix with exactly eight include rows')
    if 'uses' in cross or not {'preflight', 'await-tests'}.issubset(set(cross.get('needs') or [])):
        errors.append('release-packages.yml/build-cross: malformed job or missing release test dependencies')
    if len(owners) != 38:
        errors.append(f'catalog owns {len(owners)} distinct source workflows; expected 38')

    if errors:
        print('Actions topology FAILED:', file=sys.stderr)
        for error in errors:
            print(' - ' + error, file=sys.stderr)
        return 1
    print(
        f'Actions topology OK: {len(EXPECTED)} entrypoints, {len(owners)} source suites, '
        'inlined jobs, schedules and routing validated.'
    )
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
