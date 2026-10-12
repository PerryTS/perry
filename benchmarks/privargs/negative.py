#!/usr/bin/env python3
"""Patch only a copied lane binary for a semantic negative control."""
import argparse
import re
import shutil
import struct
import subprocess
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('source', type=Path)
p.add_argument('output', type=Path)
p.add_argument('mode', choices=['arguments', 'home', 'snapshot'])
a = p.parse_args()
if a.source.resolve() == a.output.resolve():
    raise SystemExit('the control must be a copy')
shutil.copy2(a.source, a.output)
data = bytearray(a.output.read_bytes())
if data[:6] != b'\x7fELF\x02\x01':
    raise SystemExit('expected ELF64 little endian')
phoff = struct.unpack_from('<Q', data, 32)[0]
entsize, count = struct.unpack_from('<HH', data, 54)
segments = [struct.unpack_from('<IIQQQQQQ', data, phoff + i * entsize) for i in range(count)]

def offset(address):
    for kind, flags, start, virtual, physical, size, memory, align in segments:
        if kind == 1 and virtual <= address < virtual + size:
            return start + address - virtual
    raise ValueError(address)

nm = subprocess.check_output(['nm', '-n', str(a.source)], text=True)
symbols = {parts[2]: int(parts[0], 16) for line in nm.splitlines()
           if len(parts := line.split()) == 3 and re.fullmatch('[0-9a-fA-F]+', parts[0])}
assembly = subprocess.check_output(['objdump', '-d', str(a.source)], text=True)
function = ''
changed = 0
for line in assembly.splitlines():
    header = re.match(r'^[0-9a-f]+ <([^>]+)>:', line)
    if header:
        function = header[1]
    instruction = re.match(r'^\s*([0-9a-f]+):\s+(e8(?: [0-9a-f]{2}){4})\s+call\s+[0-9a-f]+ <([^>]+)>', line)
    if not instruction:
        continue
    address, target = int(instruction[1], 16), instruction[3]
    at = offset(address)
    if ((a.mode == 'arguments' and target == 'js_arguments_object_alloc')
            or (a.mode == 'snapshot' and target == 'js_arguments_bundle_index_get')):
        replacement = symbols['js_nanbox_get_pointer']
        data[at + 1:at + 5] = struct.pack('<i', replacement - address - 5)
        changed += 1
    elif a.mode == 'home' and function.startswith('perry_method_') and function.endswith('__eclo') and target in ['js_class_method_entry_enter_home', 'js_class_method_entry_leave']:
        data[at:at + 5] = b'\x90' * 5
        changed += 1
if not changed:
    raise SystemExit('no subject instruction changed')
a.output.write_bytes(data)
print('changed calls:', changed)
