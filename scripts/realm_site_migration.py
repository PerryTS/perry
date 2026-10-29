#!/usr/bin/env python3
"""Shared placement step: wrap an existing allocator expression with the declarations of the entries it births."""
import collections
import json
import re
from pathlib import Path


def companions(root):
    return {f: json.loads(f.read_text()) for f in root.glob('crates/*/src/**/*.realm.json')}


def pending(docs):
    return {d['name']: d for doc in docs.values() for entries in doc['groups'].values() for d in entries}


def locate(source, expression, anchor=None, index=0):
    start = source.index(anchor) if anchor else 0
    found = [x.start() for x in re.finditer(re.escape(expression), source[start:])]
    return start + found[index]


def retire(path, docs, moved):
    doc = docs[path]
    for group, entries in doc['groups'].items():
        doc['groups'][group] = [d for d in entries if d['name'] not in moved]
    if any(doc['groups'].values()):
        path.write_text(json.dumps(doc, ensure_ascii=False, indent=2) + '\n')
    else:
        path.unlink()
        owner = path.with_name(path.name.replace('.realm.json', '.rs'))
        pattern = re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("' + re.escape(path.name) + r'#\*"\);[^\n]*$', re.M)
        source, n = pattern.subn('', owner.read_text())
        assert n == 1
        owner.write_text(source.rstrip() + '\n')


def place(root, baseline, output, sites, render, report_name, apply=False):
    """sites: (file, expression, anchor, index, [entry names]); render(entry) -> declaration text."""
    oracle = {d['name']: d for d in baseline['declarations']}
    docs = companions(root)
    todo = pending(docs)
    edits = collections.defaultdict(list)
    placements = []
    for file, expression, anchor, index, names in sites:
        decls = [todo[n] for n in names if n in todo]
        if not decls:
            continue
        assert len(decls) == len(names), (file, names)
        source = (root / file).read_text()
        offset = locate(source, expression, anchor, index)
        text = 'crate::realm_decl! { birth ' + expression + '; realm { ' + '; '.join(render(d) for d in decls) + '; } }'
        edits[file].append((offset, offset + len(expression), text))
        for d in decls:
            old = oracle[d['name']]
            placements.append(dict(name=d['name'], source=file, line=source[:offset].count('\n') + 1,
                                   old_source='crates/' + old['source'], old_line=old['line']))
    if apply:
        for file, rs in edits.items():
            path = root / file
            source = path.read_text()
            for a, b, text in sorted(rs, reverse=True):
                source = source[:a] + text + source[b:]
            path.write_text(source)
        moved = {p['name'] for p in placements}
        for path in docs:
            retire(path, docs, moved)
    report = dict(placements=placements, entries_moved=len(placements), sites=sum(map(len, edits.values())),
                  cross_file_entries=sum(p['source'] != p['old_source'] for p in placements))
    output.mkdir(parents=True, exist_ok=True)
    (output / report_name).write_text(json.dumps(report, indent=2) + '\n')
    print({k: v for k, v in report.items() if k != 'placements'})
    return report
