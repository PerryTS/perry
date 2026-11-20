#!/usr/bin/env python3
"""openers.py DUMP [KEY]: the first write that opened each native-write gate (`birth-opener`).

Prints one TSV row per gate in opening order: seq, holder token, holder name, key (`*` = every key),
what opened it, the store site and its function, the receiver node and the root of its TOP chain.
With KEY: only the gates a read of KEY on a builtin prototype passes (key KEY or every key, on any
holder); the first row is the write that opened that read's gate."""
import json, sys

def openers(path):
    with open(path, 'rb') as f:
        for l in f:
            if l.startswith(b'D {') and b'"record":"birth-opener"' in l:
                yield json.loads(l[2:])

def main():
    path = sys.argv[1]
    want = sys.argv[2] if len(sys.argv) > 2 else None
    rows = sorted(openers(path), key=lambda r: r['seq'])
    if want is not None:
        rows = [r for r in rows if r['key'] in (None, want)]
    print('seq\tholder\tholder_name\tkey\twhat\tsite\tsite_function\tsite_desc\trecv\trecv_root\trecv_why')
    for r in rows:
        cell = lambda v: '' if v is None else str(v).replace('\t', ' ').replace('\n', ' ')
        print('\t'.join(cell(x) for x in (r['seq'], r['holder'], r['holderName'], '*' if r['key'] is None else r['key'],
            r['what'], r['site'], r['siteFunction'], r['siteDesc'], r['recv'], r['recvRoot'], r['recvWhy'])))

if __name__ == '__main__':
    main()
