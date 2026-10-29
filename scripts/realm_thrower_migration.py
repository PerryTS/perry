#!/usr/bin/env python3
"""Declare the %ThrowTypeError% singleton at its fn_info site: frozen, anonymous."""
import argparse
import json
import re
from pathlib import Path
import realm_site_migration as s

FILE = 'crates/perry-runtime/src/object/arguments.rs'
NAME = 'Function.prototype.arguments.<get>'
OLD = 'crate::fn_info!(arguments_throw_type_error, 0; with_declared(0))'
ATTR = '#[realm("' + NAME + '", integrity = frozen, spec = {name = ""}, emit = metadata(realm_thrower_public))]'
NAME_LITERAL = 'super::native_module::set_bound_native_closure_name(closure, "");'


def rewrite(source):
    assert source.count(OLD) == 1 and source.count(NAME_LITERAL) == 1
    source = source.replace(NAME_LITERAL, 'super::native_module::set_bound_native_closure_name(closure, realm_thrower_public!(name));')
    return source.replace(OLD, 'crate::fn_info!(' + ATTR + ' arguments_throw_type_error, 0; with_declared(0))')


def migrate(root, baseline, output, apply=False):
    oracle = {d['name']: d for d in baseline['declarations']}
    docs = s.companions(root)
    if NAME not in s.pending(docs):
        return None
    path = root / FILE
    source = path.read_text()
    line = source[:source.index(OLD)].count('\n') + 1
    if apply:
        path.write_text(rewrite(source))
        for p in docs:
            s.retire(p, docs, {NAME})
    old = oracle[NAME]
    report = dict(placements=[dict(name=NAME, source=FILE, line=line, old_source='crates/' + old['source'], old_line=old['line'])],
                  entries_moved=1)
    output.mkdir(parents=True, exist_ok=True)
    (output / 'migration-thrower.json').write_text(json.dumps(report, indent=2) + '\n')
    print({k: v for k, v in report.items() if k != 'placements'})
    return report


if __name__ == '__main__':
    a = argparse.ArgumentParser(description=__doc__)
    a.add_argument('baseline', type=Path)
    a.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    a.add_argument('--output', type=Path, required=True)
    a.add_argument('--apply', action='store_true')
    a = a.parse_args()
    migrate(a.root, json.loads(a.baseline.read_text()), a.output, a.apply)
