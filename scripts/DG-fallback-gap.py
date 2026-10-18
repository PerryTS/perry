#!/usr/bin/env python3
"""Targeted fallback sidecar acceptance: no instrumentation when disabled.

Usage: DG-fallback-gap.py /path/to/perry [--fixture path] [--out directory]
The same output path, runtime archive and source are used for every compile.
"""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import struct
import shutil
import subprocess
import tempfile


def section(binary, name):
    raw = binary.read_bytes()
    assert raw[:6] == b'\x7fELF\x02\x01', 'acceptance currently requires little-endian ELF64'
    offset = struct.unpack_from('<Q', raw, 40)[0]
    stride, count, names_index = struct.unpack_from('<HHH', raw, 58)
    headers = [struct.unpack_from('<IIQQQQIIQQ', raw, offset+i*stride) for i in range(count)]
    names = headers[names_index]
    names = raw[names[4]:names[4]+names[5]]
    for h in headers:
        if names[h[0]:].split(b'\0', 1)[0].decode() == name:
            return raw[h[4]:h[4]+h[5]]
    return None


def linkage_guid(name):
    return int.from_bytes(hashlib.md5(name.encode()).digest()[:8], 'little')


def decode_probes(raw, symbols):
    anchors = {}
    for name, address in symbols.items():
        guid = linkage_guid(name)
        if guid in anchors and anchors[guid] != address:
            raise ValueError('ambiguous linkage GUID')
        anchors[guid] = address
    pos = 0
    last = None
    result = []

    def fixed(n):
        nonlocal pos
        if pos + n > len(raw):
            raise ValueError('truncated pseudo-probe data')
        out = int.from_bytes(raw[pos:pos+n], 'little')
        pos += n
        return out

    def leb(signed=False):
        value = shift = 0
        while True:
            byte = fixed(1)
            value |= (byte & 127) << shift
            shift += 7
            if shift > 70:
                raise ValueError('oversized pseudo-probe LEB')
            if not byte & 128:
                if signed and byte & 64:
                    value -= 1 << shift
                return value

    def function(depth=0):
        nonlocal last
        if depth > 128:
            raise ValueError('excessive pseudo-probe inline depth')
        guid, count, inlinees = fixed(8), leb(), leb()
        if depth == 0:
            last = anchors.get(guid)
        for _ in range(count):
            index, flags = leb(), fixed(1)
            kind, attr, delta = flags & 15, (flags >> 4) & 7, flags & 128
            if kind > 2:
                raise ValueError('unsupported pseudo-probe kind')
            if attr & 2:
                if delta:
                    raise ValueError('unexpected relative sentinel')
                last = anchors.get(fixed(8))
            elif delta:
                offset = leb(signed=True)
                if last is not None:
                    last += offset
            else:
                last = fixed(8)
            discriminator = leb() if attr & 4 else 0
            if not attr & 2:
                result.append(dict(guid=guid, index=index, pc=last,
                                   kind=kind, discriminator=discriminator))
        for _ in range(inlinees):
            leb()  # The inline callsite ID; the record retains its own GUID.
            function(depth+1)

    while pos < len(raw):
        function()
    return result


def check_native_calls(binary, data):
    """An LLVM block-entry label must never pass as a helper call-site label."""
    raw = subprocess.check_output(['objdump','-d','-w','--no-show-raw-insn',str(binary)], text=True)
    calls={}
    instructions={}
    for line in raw.splitlines():
        instruction=re.match(r'\s*([0-9a-f]+):\s+(.+)',line)
        if instruction:instructions[int(instruction[1],16)]=instruction[2]
        m=re.match(r'\s*([0-9a-f]+):\s+(?:callq?|jmpq?)\s+.*<([^>]+)>',line)
        if m:calls[int(m[1],16)]=m[2].split('+')[0].split('@')[0]
    symbols={name: value['address'] for name,value in data['symbols'].items()}
    labels={}
    for section in data['probe_sections']:
        for probe in decode_probes(bytes.fromhex(section['data_hex']),symbols):
            if probe['pc'] is not None:
                labels.setdefault((probe['guid'],probe['index']),[]).append(probe)
    checked=set()
    addresses=sorted(instructions)
    positions={pc:i for i,pc in enumerate(addresses)}
    for record in data['records']:
        for label in record['labels']:
            for probe in labels.get((linkage_guid(label['function']),label['index']),[]):
                assert probe['kind'] in (1,2), ('block probe used for call',record)
                assert probe['pc'] in positions, ('label is not an instruction boundary',probe)
                for pc in addresses[positions[probe['pc']]:]:
                    inst=instructions[pc]
                    if re.match(r'(?:callq?|jmpq?|j\w+|retq?|loop\w*)\b',inst):
                        assert calls.get(pc)==record['helper'], (hex(probe['pc']),hex(pc),calls.get(pc),record['helper'])
                        checked.add(pc)
                        break
                else:raise AssertionError(('no call after label',probe))
    expected={pc for pc,helper in calls.items() if helper in {r['helper'] for r in data['records']}}
    # Restrict to generated functions: the runtime calls itself without passing
    # through the compiler's generic-emission decisions.
    spans=[(v['address'],v['address']+v['size']) for n,v in data['symbols'].items()
           if n.startswith('perry_fn_') or n=='main' or n.startswith('__perry_cjs_')]
    expected={pc for pc in expected if any(lo<=pc<hi for lo,hi in spans)}
    assert expected and expected<=checked, ('unlabeled native fallback calls', [hex(pc) for pc in expected-checked])
    return len(checked)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('perry', type=Path)
    parser.add_argument('--fixture', type=Path, default=Path(__file__).parent.parent/'test-files/test_gap_fallback_diagnostics.ts')
    parser.add_argument('--out', type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='DG-fallback-') as temp:
        root = args.out or Path(temp)
        root.mkdir(parents=True, exist_ok=True)
        binary = root/'program'
        sidecar = root/'program.fallback.json'
        sidecar.unlink(missing_ok=True)
        env = dict(os.environ, PERRY_KEEP_SYMBOLS='1')
        env.pop('PERRY_FALLBACK_DIAG', None)
        command = [str(args.perry.resolve()), 'compile', '--no-auto-optimize',
                   '--cache-dir', str(root/'cache'), str(args.fixture.resolve()), '-o', str(binary)]
        def compile(label, value):
            if value is not None:
                env['PERRY_FALLBACK_DIAG'] = value
            with (root/(label+'.compile.log')).open('w') as log:
                subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
            return binary.read_bytes(), subprocess.check_output([str(binary)])
        baseline, output = compile('unset', None)
        assert not sidecar.exists(), 'unset diagnostics produced a sidecar'
        zero, zero_output = compile('zero', '0')
        assert zero == baseline, 'PERRY_FALLBACK_DIAG=0 changed the default binary'
        assert zero_output == output
        baseline_text_sha256 = hashlib.sha256(section(binary, '.text')).hexdigest()
        diagnostic, diagnostic_output = compile('on', '1')
        diagnostic_text_sha256 = hashlib.sha256(section(binary, '.text')).hexdigest()
        assert diagnostic_output == output
        oracle = subprocess.check_output([os.environ.get('NODE', shutil.which('node') or '/usr/bin/node'), str(args.fixture.resolve())])
        assert output == oracle, (output, oracle)
        data = json.loads(sidecar.read_text())
        assert data['schema'] == 1 and data['records'] and data['probe_sections']
        native_calls=check_native_calls(binary,data)
        records = data['records']
        assert any(r['operation'] == 'store' and r['approximate'] for r in records)
        assert any(r['key'] == 'computed' and r['operation']=='computed store' for r in records)
        assert any(r['operation']=='has' for r in records)
        assert any(r['helper'].startswith('js_closure_call') and 'direct entry' in r['reason'] for r in records)
        assert any(r['operation']=='birth' and r['receiver']['classes'] for r in records)
        assert any(r['line'] and not r['approximate'] for r in records)
        assert any(r['receiver']['cause'] for r in records)
        assert all(r['reason'] and r['guard'] and r['compiler_line'] for r in records)
        assert section(binary, '.pseudo_probe') is None, 'probe table leaked into binary'
        assert section(binary, '.pseudo_probe_desc') is None, 'probe descriptors leaked into binary'
        # A second run must regenerate records even with a warm cache/output.
        _, repeated_output = compile('repeat', '1')
        repeated = json.loads(sidecar.read_text())
        assert repeated_output == output and len(repeated['records']) == len(records)
        # Retire the sidecar before checking explicit OFF again: disabling
        # diagnostics never deletes an artifact owned by an earlier invocation.
        sidecar.unlink()
        after, _ = compile('off-again', '0')
        assert after == baseline and not sidecar.exists()
        result = dict(parity='IDENTICAL', off_binary_sha256=hashlib.sha256(baseline).hexdigest(),
                      records=len(records), native_calls_verified=native_calls, probe_sections_removed=True,
                      on_text_equal=baseline_text_sha256==diagnostic_text_sha256,
                      text_bytes=len(section(binary, '.text')))
        (root/'DG-gap-result.json').write_text(json.dumps(result, indent=2)+'\n')
        print(json.dumps(result))


if __name__ == '__main__':
    main()
