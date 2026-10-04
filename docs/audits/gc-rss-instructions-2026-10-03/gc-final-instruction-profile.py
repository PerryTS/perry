"""Attribute remaining GC instructions in already verified normal-auto binaries.
Profiles are diagnostics, never substituted for plain peak RSS or perf-stat runs.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess


def parse_report(text):
    lost = re.search(r'^# Total Lost Samples:\s*(\d+)\s*$', text, re.M)
    events = re.search(r'^# Event count \(approx\.\):\s*(\d+)\s*$', text, re.M)
    if not lost or int(lost[1]) != 0 or not events or int(events[1]) <= 0:
        raise ValueError('missing event count or lost instruction samples')
    rows = []
    for line in text.splitlines():
        fields = [s.strip() for s in line.split('|')]
        if not fields or not re.fullmatch(r'\d+\.\d+%', fields[0]):
            continue
        if len(fields) < 4 or not fields[1].isdigit():
            raise ValueError('unrecognized perf report row')
        rows.append(dict(percent=float(fields[0][:-1]), samples=int(fields[1]),
                         dso=fields[2], symbol=fields[3]))
    total = sum(row['samples'] for row in rows)
    if not rows or total <= 0:
        raise ValueError('empty instruction attribution')
    gc = sum(r['samples'] for r in rows if 'perry_runtime::gc::' in r['symbol'])
    arena = sum(r['samples'] for r in rows if 'perry_runtime::arena::' in r['symbol'])
    unknown = sum(r['samples'] for r in rows if r['dso'] == '[unknown]' or
                  re.match(r'^\[.\] 0x[0-9a-f]+$', r['symbol']))
    return dict(event_count_approx=int(events[1]), samples=total, lost_samples=0,
                gc_self_samples=gc, arena_self_samples=arena,
                unresolved_samples=unknown, top_symbols=rows[:40])


def run():
    import bench
    B = bench.B
    version = 'gc-final-instruction-profile-v1'
    assert not (B / (version + '-runs.json')).exists(), 'preserve completed or partial evidence'
    plans = [
        ('gc-auto-v2', 'gc-runtime-scan-v2-auto.exit',
         ['mainrefresh-base', 'gc-runtime-scan-main'], ['30-string-build']),
        ('gc-auto-main69', 'gc-runtime-main69r1-auto.exit',
         ['main69-base', 'main69-gc'], ['tscwork', 'zodwork', '30-string-build']),
    ]
    cases = {name: (source, args) for name, source, args in bench.CASES}
    prepared = []
    # Refuse all diagnostic runs until every input binary and matrix is verified.
    for prefix, receipt, arms, names in plans:
        assert (B / receipt).read_text().strip() == '0', receipt
        oracle_path = B / (prefix + '-oracle.json')
        oracles = json.loads(oracle_path.read_text())
        build_path = B / (prefix + '-builds.json')
        builds = json.loads(build_path.read_text())
        measured = json.loads((B / (prefix + '-runs.json')).read_text())
        for name in names:
            source, args = cases[name]
            for arm in arms:
                plain = B / 'bin' / f'{prefix}-{arm}-{source.stem}'
                symbols = Path(str(plain) + '-symbols')
                matches = [r for r in builds if r.get('arm') == arm and
                           r.get('source') == str(source) and r.get('rc') == 0]
                assert len(matches) == 1, (prefix, name, arm)
                build = matches[0]
                assert build['source_sha256'] == bench.m.sha(source)
                assert build['binary_sha256'] == bench.m.sha(plain)
                assert build['symbol_sha256'] == bench.m.sha(symbols)
                for binary in [plain, symbols]:
                    section = B / f'{version}-{binary.name}.text'
                    subprocess.run(['objcopy', '--dump-section', f'.text={section}', binary], check=True)
                    assert bench.m.sha(section) == build['text_sha256']
                    section.unlink()  # Reproducible temporary section, not a measurement.
                cells = [r for r in measured if r['case'] == name and r['arm'] == arm]
                assert {(r['mode'], r['repeat']) for r in cells} == {
                    (mode, rep) for mode in ['plain', 'perf'] for rep in range(3)}
                assert len(cells) == 6
                assert all(r['rc'] == 0 and r['reason'] is None and
                           r['stdout'] == oracles[name] and
                           r['binary_sha256'] == build['binary_sha256'] for r in cells)
                prepared.append(dict(prefix=prefix, case=name, arm=arm, args=args,
                                     symbols=str(symbols), expected_stdout=oracles[name],
                                     symbol_sha256=build['symbol_sha256'],
                                     plain_sha256=build['binary_sha256'],
                                     text_sha256=build['text_sha256'],
                                     build_manifest_sha256=bench.m.sha(build_path),
                                     oracle_sha256=bench.m.sha(oracle_path)))
    records = []
    for repeat in range(2):
        for subject in prepared if repeat == 0 else reversed(prepared):
            label = f"{version}-{subject['prefix']}-{subject['arm']}-{subject['case']}-{repeat}"
            data = B / (label + '.perf.data')
            assert not data.exists(), data
            record = bench.run(label, ['taskset', '-c', '2', 'perf', 'record', '-e',
                'instructions:u', '-c', '1000000', '-o', data, '--',
                subject['symbols'], *subject['args']], timeout=300)
            record.update(subject, repeat=repeat)
            record['correct'] = record['rc'] == 0 and record['reason'] is None and record['stdout'] == subject['expected_stdout']
            records.append(record)
            bench.m.save(version + '-runs.json', records)
            assert record['correct'], label
            assert bench.m.sha(subject['symbols']) == subject['symbol_sha256']
            report = subprocess.check_output(['perf', 'report', '-i', data, '--stdio',
                '--no-children', '--show-nr-samples', '--sort', 'dso,symbol',
                '--percent-limit', '0', '--field-separator', '|'], text=True)
            report_path = B / (label + '.report.txt')
            report_path.write_text(report)
            record.update(attribution=parse_report(report), data_sha256=bench.m.sha(data),
                          report_sha256=bench.m.sha(report_path))
            bench.m.save(version + '-runs.json', records)
            print(subject['arm'], subject['case'], repeat, record['attribution']['samples'], 'samples; correct', flush=True)
    assert len(records) == 16
    summary = [{k: r[k] for k in ['prefix', 'arm', 'case', 'repeat', 'correct',
                'symbol_sha256', 'plain_sha256', 'text_sha256', 'attribution',
                'data_sha256', 'report_sha256']} for r in records]
    bench.m.save(version + '-summary.json', dict(
        scope='User instruction self samples; approximate attribution, not exact instruction deltas, elapsed time, or peak RSS.',
        records=summary))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--parse-report', type=Path)
    args = parser.parse_args()
    if args.parse_report:
        result = parse_report(args.parse_report.read_text())
        print(json.dumps(result, indent=2))
    else:
        run()
