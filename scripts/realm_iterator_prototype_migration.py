#!/usr/bin/env python3
"""Declare the four family iterator prototypes at the one allocator that births them."""
import argparse
import json
from pathlib import Path
import realm_prototype_birth_migration as p
import realm_site_migration as s

FAMILIES = ['%' + n + 'IteratorPrototype%' for n in ('Array', 'Map', 'Set', 'String')]
FILE = 'crates/perry-runtime/src/object/iterator_prototypes.rs'


def sites():
    return [(FILE, 'js_object_alloc(0, 0)', 'fn build_family_proto(', 0, FAMILIES)]


def render(d):
    """The runtime marks these never-move when iterators adopt them; the declaration owns that fact."""
    return p.render(d) + ', never_move = true'


def migrate(root, baseline, output, apply=False):
    return s.place(root, baseline, output, sites(), render, 'migration-iterator-prototypes.json', apply)


if __name__ == '__main__':
    a = argparse.ArgumentParser(description=__doc__)
    a.add_argument('baseline', type=Path)
    a.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    a.add_argument('--output', type=Path, required=True)
    a.add_argument('--apply', action='store_true')
    a = a.parse_args()
    migrate(a.root, json.loads(a.baseline.read_text()), a.output, a.apply)
