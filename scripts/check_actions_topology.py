#!/usr/bin/env python3
"""Check workflow entrypoints, reusable calls, routing catalog and trigger unions."""
from __future__ import annotations
import json
import sys
from pathlib import Path
import yaml

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / '.github/workflows'
CATALOG = json.loads((ROOT / 'scripts/actions_catalog.json').read_text())
EXPECTED = {
    'test.yml', 'gc.yml', 'compiler-runtime.yml', 'compatibility.yml',
    'integration.yml', 'performance.yml', 'documentation.yml',
    'release-packages.yml', 'release-hono-server.yml', 'maintenance.yml',
}

def read(path: Path):
    return yaml.load(path.read_text(), Loader=yaml.BaseLoader) or {}

def trigger_set(workflow):
    triggers = workflow.get('on', workflow.get(True, {}))
    if isinstance(triggers, list): return set(triggers)
    return set(triggers) if isinstance(triggers, dict) else set()

def main() -> int:
    errors=[]; owners={}; parents={}; graph={}
    workflows={p.name:read(p) for p in WORKFLOWS.glob('*.yml')}
    active={name for name,w in workflows.items() if any(e!='workflow_call' for e in trigger_set(w))}
    if active != EXPECTED:
        errors.append(f'entrypoints mismatch: missing={sorted(EXPECTED-active)} extra={sorted(active-EXPECTED)}')
    for key,category in CATALOG['categories'].items():
        parent=category['entrypoint']; parents[key]=parent
        if parent not in workflows: errors.append(f'{key}: missing parent {parent}'); continue
        wf=workflows[parent]; triggers=trigger_set(wf); jobs=wf.get('jobs',{})
        concurrency=wf.get('concurrency') or {}
        parent_group=str(concurrency.get('group') or '')
        if key not in {'ci', 'release-packages', 'release-hono-server'}:
            if 'github.run_id' not in parent_group: errors.append(f'{parent}: non-PR concurrency must use github.run_id')
            if "github.event_name == 'pull_request'" not in str(concurrency.get('cancel-in-progress') or ''):
                errors.append(f'{parent}: only PR runs may supersede one another')
        graph[parent]=[]
        seen_crons=set()
        for module in category['modules']:
            file=module['file']; mid=module['id']
            if file in owners: errors.append(f'{file}: owned by both {owners[file]} and {key}')
            owners[file]=key
            child=workflows.get(file)
            if child is None: errors.append(f'{key}/{mid}: missing child {file}'); continue
            child_triggers=trigger_set(child)
            if file != parent and child_triggers != {'workflow_call'}: errors.append(f'{file}: expected workflow_call only, got {sorted(child_triggers)}')
            child_group=str(((child.get('concurrency') or {}).get('group')) or '')
            if file != parent and parent_group and child_group and parent_group == child_group:
                errors.append(f'{parent} and {file}: caller and child concurrency groups collide')
            if parent==file: continue
            caller=jobs.get(mid)
            if parent == 'test.yml' and mid == 'security-audit':
                caller=jobs.get('security-weekly')
            if not caller: errors.append(f'{parent}: missing caller job {mid}'); continue
            expected_use=f'./.github/workflows/{file}'
            if caller.get('uses') != expected_use: errors.append(f'{parent}/{mid}: expected uses {expected_use}')
            if not (parent == 'test.yml' and mid == 'security-audit') and caller.get('name',mid) != mid: errors.append(f'{parent}/{mid}: caller display name must equal id')
            for nested in module.get('required_jobs',[]):
                # The manifest sometimes records the child job's stable API name
                # while the local job key has changed over time. Require a
                # matching key or explicit display name.
                child_jobs=child.get('jobs') or {}
                if nested not in child_jobs and not any((job or {}).get('name') == nested for job in child_jobs.values()): errors.append(f'{file}: required nested job {nested} missing')
            graph[parent].append(file)
            declared=(child.get('on') or {}).get('workflow_call') or {}
            declared_inputs=(declared.get('inputs') or {})
            caller_inputs=caller.get('with') or {}
            for input_name in caller_inputs:
                if input_name not in declared_inputs: errors.append(f'{parent}/{mid}: undeclared child input {input_name}')
            for input_name,spec in module.get('inputs',{}).items():
                mapping=module.get('parent_input_map',{}).get(input_name,input_name)
                if spec.get('type') == 'secret':
                    if input_name not in (caller.get('secrets') or {}): errors.append(f'{parent}/{mid}: missing secret forwarding for {input_name}')
                    continue
                if input_name not in declared_inputs: errors.append(f'{file}: catalog input {input_name} is not declared')
                if input_name not in caller_inputs and spec.get('type') != 'secret':
                    errors.append(f'{parent}/{mid}: missing typed input forwarding for {input_name}')
            for cron in [row['cron'] for row in module.get('original_events',{}).get('schedule',[])]:
                seen_crons.add(cron)
                if 'schedule' not in triggers: errors.append(f'{parent}: missing schedule trigger {cron}')
        # All original schedule values from source inventory/catalog must exist exactly once.
        actual={row.get('cron') for row in (wf.get('on') or {}).get('schedule',[]) if isinstance(row,dict)}
        expected={row['cron'] for m in category['modules'] for row in m.get('original_events',{}).get('schedule',[])}
        if actual != expected: errors.append(f'{parent}: schedule mismatch missing={sorted(expected-actual)} extra={sorted(actual-expected)}')
    cycles=[]
    def visit(node, stack, done):
        if node in stack: cycles.append(' -> '.join(stack+[node])); return
        if node in done: return
        for nxt in graph.get(node,[]): visit(nxt,stack+[node],done)
        done.add(node)
    done=set()
    for node in graph: visit(node,[],done)
    errors.extend('workflow call cycle: '+c for c in cycles)
    for file in ('simctl-tests.yml', 'container-tests.yml', 'coverage.yml'):
        child=workflows.get(file) or {}
        policy=child.get('concurrency') or {}
        group=str(policy.get('group') or '')
        cancel=str(policy.get('cancel-in-progress') or '')
        if 'github.run_id' not in group:
            errors.append(f'{file}: non-PR runs must have a unique concurrency group')
        if cancel not in ('false', '${{ github.event_name == \'pull_request\' }}'):
            errors.append(f'{file}: only PR runs may cancel prior runs')
    # Protect the cross-compile release matrix from the malformed job-level
    # `uses` regression that previously made GitHub reject the workflow.
    release=workflows.get('release-packages.yml') or {}
    cross=((release.get('jobs') or {}).get('build-cross') or {})
    matrix=((cross.get('strategy') or {}).get('matrix') or {})
    rows=matrix.get('include') or []
    if set(matrix) != {'include'} or not isinstance(rows,list) or len(rows) != 8:
        errors.append('release-packages.yml/build-cross: expected matrix with exactly eight include rows')
    if 'uses' in cross or not {'preflight','await-tests'}.issubset(set(cross.get('needs') or [])):
        errors.append('release-packages.yml/build-cross: malformed job or missing release test dependencies')
    # Entry files contain broad original trigger groups; no other child may carry events.
    if len(owners)!=38: errors.append(f'catalog owns {len(owners)} distinct source files; expected 38')
    if errors:
        print('Actions topology FAILED:',file=sys.stderr)
        for error in errors: print(' - '+error,file=sys.stderr)
        return 1
    print(f'Actions topology OK: {len(EXPECTED)} entrypoints, {len(owners)} uniquely owned workflows, schedules and reusable calls validated.')
    return 0

if __name__=='__main__': raise SystemExit(main())
