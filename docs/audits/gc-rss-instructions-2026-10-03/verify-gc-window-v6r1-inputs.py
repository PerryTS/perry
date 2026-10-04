"""Bind a timeout repair to unchanged, successfully validated frozen products."""
import hashlib
import json
from pathlib import Path
import re


def sha(p):
    h = hashlib.sha256()
    with p.open('rb') as stream:
        for block in iter(lambda: stream.read(1048576), b''):
            h.update(block)
    return h.hexdigest()


def main():
    b = Path(__file__).resolve().parent
    assert (b / 'gc-runtime-window-v6-run.exit').read_text().strip() == '1'
    for name in ['gc-runtime-window-v6-build.exit', 'gc-runtime-window-v6-linked.exit']:
        assert (b / name).read_text().strip() == '0', name
    failed_path = b / 'gc-window-v6-noauto-builds.json'
    failed = json.loads(failed_path.read_text())
    assert len(failed) == 2
    control, candidate = failed
    assert control['arm'] == 'main69-gc' and control['case'] == 'tscwork'
    assert control['rc'] == 0 and control['reason'] is None
    assert candidate['arm'] == 'main69-window' and candidate['case'] == 'tscwork'
    assert candidate['rc'] == -9 and candidate['reason'] == 'timeout'
    assert candidate['elapsed_s'] >= 600
    assert not (b / 'gc-window-v6-noauto-runs.json').exists()
    assert not (b / 'gc-auto-window-v6-runs.json').exists()
    binary = b / 'bin/gc-window-v6-noauto-main69-gc-tscwork'
    assert sha(binary) == control['binary_sha256']
    assert sha(Path(control['cmd'][2])) == control['source_sha256']
    products = {}
    for name in ['gc-runtime-window-v6-input-copy.json', 'gc-runtime-window-v6-products.json']:
        data = json.loads((b / name).read_text())
        products.update(data['original_products'] if 'original_products' in data else data)
    for path, digest in products.items():
        assert sha(Path(path)) == digest, path
    source = json.loads((b / 'provenance-gc-runtime-window-v6.json').read_text())
    for path, digest in source['files'].items():
        assert sha(b / 'perry-main69-window' / path) == digest, path
    logs = {}
    for label, expected in [('linux', 4834), ('ffi', 75), ('events', 15)]:
        # Names are pinned by the original, successful build driver.
        names = {'linux': 'gc-runtime-window-v6-linux-tests.log',
                 'ffi': 'gc-runtime-window-v6-linux-ffi.log',
                 'events': 'gc-runtime-window-v6-linux-events.log'}
        path = b / 'logs' / names[label]
        text = path.read_text()
        assert re.search(r'test result: ok\. ' + str(expected) + r' passed; 0 failed;', text)
        logs[str(path)] = sha(path)
        if label == 'linux':
            for test in ['reused_blocks_restart_the_window_and_cold_blocks_are_advised_once',
                         'real_collection_publication_keeps_warm_pages_and_releases_unused_pages']:
                assert re.search(r'^test .*::' + test + r' \.\.\. ok$', text, re.M)
    linked_path = b / 'gc-window-v6-linked-runs.json'
    linked = json.loads(linked_path.read_text())
    executed = [r for r in linked if r['mode'] != 'build']
    assert len(executed) == 12
    assert all(r['rc'] == 0 and r['reason'] is None and r['correct'] for r in executed)
    moving = [r for r in executed if r['mode'] == 'moving']
    assert len(moving) == 6 and all(r['copied_objects'] > 0 and r['protected'] for r in moving)
    for row in executed:
        assert sha(Path(row['cmd'][0])) == row['binary_sha256']
    record = dict(scope='Original runtime/source/linked validation remains valid; failed compilation produces no RSS or instruction measurement. Retry under a new namespace with a 1800-second compile guard and fresh runs.',
                  original_failure_sha256=sha(failed_path), failed=candidate,
                  reused_control_binary=str(binary), reused_control_sha256=sha(binary),
                  products=products, source=source, logs=logs,
                  linked_manifest_sha256=sha(linked_path), moving_executions=len(moving))
    (b / 'gc-runtime-window-v6r1-input-verification.json').write_text(json.dumps(record, indent=2) + '\n')
    print('Frozen products, source, Linux/FFI/events and 12 linked executions verified; timeout preserved.')


if __name__ == '__main__':
    main()
