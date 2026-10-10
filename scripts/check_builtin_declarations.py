#!/usr/bin/env python3
"""Guard builtin installation: declarations, never name branches, choose identity/lanes.

Scan complete populate/installer modules, including sibling builtin installers.
Names can select the API surface to install, but cannot select class identities
or gate lane learning. Ignore comments and retain string tokens, then inspect
balanced control expressions/blocks rather than matching one source spelling.
"""
from pathlib import Path
import re
import sys
import unittest

ROOT = Path(__file__).resolve().parent.parent
TOKEN = re.compile(
    r'//[^\n]*|/\*.*?\*/|r(?P<hash>\#*)".*?"(?P=hash)|'
    r'"(?:\\.|[^"\\])*"|'
    r"'(?:\\.|[^'\\])'|"
    r'\b[A-Za-z_]\w*\b|=>|==|!=|[^\s]', re.S
)
AUTHORITY = re.compile(
    r'CLASS_ID_|INTRINSIC_SERIAL_|PROTO_ID_CLASS|write_identity_word|assign_intrinsic_prototype_serial|learn_.*lanes|'
    r'prototype_class|prototype_serial|prototype_identity|weak_class|constfn_lanes'
)
NAME = re.compile(r'(?:^|_)(?:name|label)(?:$|_)')


def violations(source):
    tokens = [(m.group(), m.start()) for m in TOKEN.finditer(source)
              if not m.group().startswith(('//', '/*'))]
    stack, ends = [], {}
    for i, (token, _) in enumerate(tokens):
        if token in ('{', '(', '['):
            stack.append(i)
        elif token in ('}', ')', ']') and stack:
            ends[stack.pop()] = i
    failures = []
    for i, (token, offset) in enumerate(tokens):
        if token not in ('if', 'match'):
            continue
        # The selector ends at its block, skipping matches!(...) and other
        # parenthesized selectors as a whole.
        j = i + 1
        while j < len(tokens) and tokens[j][0] not in ('{', ';'):
            j = ends.get(j, j) + 1
        if j >= len(tokens) or tokens[j][0] != '{' or j not in ends:
            continue
        selector = [t for t, _ in tokens[i + 1:j]]
        body = [t for t, _ in tokens[j + 1:ends[j]]]
        # A string-arm match, or an equality/matches! name guard. Member-name
        # API dispatch can still choose bodies; it cannot choose these facts.
        by_name = any(NAME.search(t) for t in selector) and any(
            t.startswith(('"', 'r"', 'r#')) for t in selector + body
        )
        # Receiver brand dispatch already takes the class id as an operand;
        # it is not assigning a prototype identity from its name.
        receiver_brand = ('method_name' in selector
                          and any(t in ('cid', 'receiver_cid', 'class_id') for t in selector)
                          and not any(t in ('name', 'builtin_name', 'class_name', 'prototype_name')
                                      for t in selector))
        if not by_name or receiver_brand:
            continue
        # Include the binding receiving a match result (let weak_class = ...).
        start = i - 1
        while start >= 0 and tokens[start][0] not in (';', '{', '}'):
            start -= 1
        affected = [t for t, _ in tokens[start + 1:i]] + selector + body
        if any(AUTHORITY.search(t) for t in affected):
            failures.append(source.count('\n', 0, offset) + 1)
    return failures


def modules():
    runtime = ROOT / 'crates/perry-runtime/src'
    # Discover installer modules from their functions, not a fixed file list,
    # so a newly split installer is covered automatically. Always cover the
    # entire global_this tree and its trunk/declarations.
    for path in sorted(runtime.rglob('*.rs')):
        text = path.read_text()
        if ('global_this' in str(path.relative_to(runtime)) or re.search(
                r'\bfn\s+(?:install_|populate_)\w*(?:builtin|proto|constructor)|'
                r'\bfn\s+(?:install_builtin|populate_builtin)\w*', text)):
            yield path, text


class NegativeControls(unittest.TestCase):
    def test_class_match(self):
        for name in ('name', 'builtin_name', 'prototype_label', 'class_name'):
            self.assertTrue(violations(
                f'let weak_class = match {name} {{ "WeakMap" => Some(17), _ => None }};'))
            self.assertTrue(violations(
                f'match {name} {{ "WeakMap" => CLASS_ID_WEAKMAP, _ => 0 }}'))

    def test_lane_guards(self):
        for guard in ('name == "RegExp"', '"RegExp" == name',
                      'matches!(builtin_name, "RegExp" | "Array")'):
            self.assertTrue(violations(
                f'if {guard} {{ unsafe {{ learn_object_constfn_lanes(proto, |_, _| true); }} }}'))
        self.assertTrue(violations(
            'match prototype_name { "Array" => { learn_object_constfn_lanes(p, f); }, _ => {} }'))

    def test_definition_and_generic_application(self):
        self.assertFalse(violations(
            'builtin_constructors! { "WeakMap" => INFO; CLASS_ID_WEAKMAP }'))
        self.assertFalse(violations(
            'if let Some(class) = declaration.prototype_class { write_identity_word(class, p); }'))
        self.assertFalse(violations(
            'match builtin_name { "Array" => install_proto_method(p, "values", INFO), _ => {} }'))
        self.assertFalse(violations(
            '// if name == "RegExp" { learn_object_constfn_lanes(p, f); }\n'
            'let comment = "match name { CLASS_ID_WEAKMAP }";'))

    def test_whole_module_mutations(self):
        checked = list(modules())
        self.assertGreater(len(checked), 20)
        for path, source in checked:
            self.assertFalse(violations(source), str(path))
            # Append controls beyond the original block: every whole file is
            # guarded, including future code outside today's populate body.
            for mutation in (
                'fn control(name: &str) { let weak_class = match name { "WeakMap" => Some(17), _ => None }; }',
                'fn control(name: &str) { if name == "RegExp" { learn_object_constfn_lanes(p, f); } }',
            ):
                self.assertTrue(violations(source + '\n' + mutation), str(path))


def main():
    if '--self-test' in sys.argv:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(NegativeControls)
        if not unittest.TextTestRunner().run(suite).wasSuccessful():
            return 1
    checked = list(modules())
    errors = [f'{path.relative_to(ROOT)}:{line}: name branch chooses identity/lanes'
              for path, source in checked for line in violations(source)]
    if errors:
        print('\n'.join(errors), file=sys.stderr)
        return 1
    print(f'builtin declaration invariant: {len(checked)} complete modules checked')
    return 0


if __name__ == '__main__':
    sys.exit(main())
