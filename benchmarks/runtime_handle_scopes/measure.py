"""Instruction counts and folded stacks; run with the archives used by --perry.

PERRY_RUNTIME_DIR and PERRY_NO_AUTO_OPTIMIZE=1 must identify the matching build.
The four sources include the original #10166 hoist/exec1 probes, unchanged.
"""
import argparse
import collections
import os
from pathlib import Path
import re
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--perry', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--node', default='node')
    parser.add_argument('--repeats', default=3, type=int)
    args = parser.parse_args()
    expected_node = 'v' + (Path(__file__).resolve().parents[2] / '.node-version').read_text().strip()
    actual_node = subprocess.check_output([args.node, '--version'], text=True).strip()
    assert actual_node == expected_node, f'expected Node {expected_node}, found {actual_node}'
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = os.environ | {'PERRY_KEEP_SYMBOLS': '1', 'PERRY_NO_AUTO_OPTIMIZE': '1'}
    for name in ('promises', 'json', 'hoist', 'exec1'):
        source = Path(__file__).with_name(name + '.ts')
        binary = output / name
        with (output / (name + '.compile.log')).open('w') as log:
            subprocess.run([args.perry, 'compile', str(source), '--no-auto-optimize',
                            '-o', str(binary)], env=env, stdout=log, stderr=log, check=True)
        expected = subprocess.check_output([args.node, str(source)])
        actual = subprocess.check_output([str(binary)])
        (output / (name + '.out')).write_bytes(actual)
        assert actual == expected, f'{name}: Node output mismatch'
        with (output / (name + '.runs.out')).open('w') as log:
            subprocess.run(['perf', 'stat', '-x', ';', '-r', str(args.repeats),
                            '-e', 'instructions:u', '-o', str(output / (name + '.stat')),
                            str(binary)], stdout=log, check=True)
            data = output / (name + '.perf.data')
            subprocess.run(['perf', 'record', '-q', '-e', 'instructions:u', '-c', '1000003',
                            '--call-graph', 'dwarf', '-o', str(data), '--', str(binary)],
                           stdout=log, check=True)
        process = subprocess.Popen(['perf', 'script', '-i', str(data)],
                                   stdout=subprocess.PIPE, text=True)
        stacks = collections.Counter()
        frames = []
        for line in process.stdout:
            if not line.strip():
                if frames:
                    stacks[';'.join(reversed(frames))] += 1
                frames = []
            elif line[0] in ' \t':
                parts = line.strip().split(None, 1)
                if len(parts) == 2:
                    symbol = parts[1].rsplit(' (', 1)[0]
                    frames.append(re.sub(r'\+0x[0-9a-f]+$', '', symbol))
        if frames:
            stacks[';'.join(reversed(frames))] += 1
        assert process.wait() == 0, 'perf script failed'
        with (output / (name + '.folded')).open('w') as folded:
            for stack, count in stacks.most_common():
                folded.write(f'{count} {stack}\n')
        total = sum(stacks.values())
        assert total > 0, 'no instruction samples'
        leaf = sum(count for stack, count in stacks.items()
                   if re.search(r'RuntimeHandle|runtime_handle|RUNTIME_HANDLE',
                                stack.rsplit(';', 1)[-1]))
        print(name, 'samples', total, 'handle-related leaf %', 100 * leaf / total, flush=True)


if __name__ == '__main__':
    main()
