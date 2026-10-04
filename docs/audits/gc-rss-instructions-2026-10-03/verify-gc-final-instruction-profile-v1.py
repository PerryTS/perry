"""Recheck frozen instruction samples against raw reports and current products.

This verifies attribution evidence, not exact GC instruction counts or acceptance.
"""
import hashlib
import importlib.util
import itertools
import json
from pathlib import Path
import subprocess
import tempfile


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1048576), b''):
            h.update(block)
    return h.hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    b = Path(__file__).resolve().parent
    version = 'gc-final-instruction-profile-v1'
    receipt = b / (version + '.exit')
    require(receipt.read_text().strip() == '0', 'profile driver did not finish successfully')
    runs_path = b / (version + '-runs.json')
    summary_path = b / (version + '-summary.json')
    runs = json.loads(runs_path.read_text())
    summary = json.loads(summary_path.read_text())
    expected = set()
    for prefix, arms, cases in [
        ('gc-auto-v2', ['mainrefresh-base', 'gc-runtime-scan-main'], ['30-string-build']),
        ('gc-auto-main69', ['main69-base', 'main69-gc'], ['tscwork', 'zodwork', '30-string-build']),
    ]:
        expected.update(itertools.product([prefix], arms, cases, range(2)))
    spec = importlib.util.spec_from_file_location('profile_parser', b / 'gc-final-instruction-profile.py')
    parser = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(parser)
    seen, products, inputs = set(), {}, {}
    keys = ['prefix', 'arm', 'case', 'repeat', 'correct', 'symbol_sha256',
            'plain_sha256', 'text_sha256', 'attribution', 'data_sha256', 'report_sha256']
    require(summary['records'] == [{k: row[k] for k in keys} for row in runs], 'summary differs from raw records')
    for row in runs:
        key = tuple(row[k] for k in ['prefix', 'arm', 'case', 'repeat'])
        require(key in expected and key not in seen, f'unexpected or duplicate profile: {key}')
        seen.add(key)
        require(row['rc'] == 0 and row['reason'] is None and row['correct'] is True,
                f'failed profile: {key}')
        require(row['stdout'] == row['expected_stdout'], f'output mismatch: {key}')
        prefix = row['prefix']
        for suffix, field in [('builds', 'build_manifest_sha256'), ('oracle', 'oracle_sha256')]:
            path = b / f'{prefix}-{suffix}.json'
            require(sha(path) == row[field], f'changed input: {path}')
            inputs[str(path)] = sha(path)
        oracles = json.loads((b / f'{prefix}-oracle.json').read_text())
        require(oracles[row['case']] == row['expected_stdout'], f'changed oracle: {key}')
        report = b / (row['label'] + '.report.txt')
        data = b / (row['label'] + '.perf.data')
        require(sha(report) == row['report_sha256'], f'changed report: {key}')
        require(sha(data) == row['data_sha256'], f'changed perf data: {key}')
        require(parser.parse_report(report.read_text()) == row['attribution'], f'attribution differs: {key}')
        symbols = Path(row['symbols'])
        plain = Path(str(symbols).removesuffix('-symbols'))
        if str(symbols) not in products:
            require(sha(symbols) == row['symbol_sha256'], f'changed symbols: {symbols}')
            require(sha(plain) == row['plain_sha256'], f'changed executable: {plain}')
            for binary in [symbols, plain]:
                with tempfile.TemporaryDirectory(prefix='perry-profile-verify-') as tmp:
                    section = Path(tmp) / 'text'
                    subprocess.run(['objcopy', '--dump-section', f'.text={section}', str(binary)], check=True)
                    require(sha(section) == row['text_sha256'], f'changed text: {binary}')
            products[str(symbols)] = {k: row[k] for k in ['symbol_sha256', 'plain_sha256', 'text_sha256']}
        require(products[str(symbols)] == {k: row[k] for k in products[str(symbols)]},
                f'product changed between profiles: {key}')
        builds = json.loads((b / f'{prefix}-builds.json').read_text())
        matches = [r for r in builds if r['arm'] == row['arm'] and
                   r.get('binary_sha256') == row['plain_sha256'] and
                   r.get('symbol_sha256') == row['symbol_sha256'] and
                   r.get('text_sha256') == row['text_sha256'] and r['rc'] == 0]
        require(len(matches) == 1, f'profile has no unique successful build: {key}')
        require(sha(Path(matches[0]['source'])) == matches[0]['source_sha256'], f'changed source: {key}')
    require(seen == expected and len(runs) == 16, 'incomplete profile matrix')
    result = dict(scope=summary['scope'], records=len(runs), products=products, inputs=inputs,
                  receipt=dict(path=str(receipt), sha256=sha(receipt)),
                  runs_sha256=sha(runs_path), summary_sha256=sha(summary_path))
    output = b / (version + '-verification.json')
    output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
