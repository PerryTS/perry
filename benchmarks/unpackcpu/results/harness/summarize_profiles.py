"""Classify sampled instructions even when assembly has no usable call chain."""
from collections import Counter, defaultdict
from pathlib import Path
import json
import re

ROOT = Path('/root/lanes/perry-unpackcpu')

def group(frames):
    leaf = frames[0] if frames else ''
    stack = ' '.join(frames)
    if re.search(r'perry_runtime::gc::|v8::internal::.*(?:CollectGarbage|MarkCompact|Scavenge|GarbageCollector|Evacuate|MarkingVisitor|IncrementalMarking|Sweeper)', stack):
        return 'GC / pacing'
    if re.search(r'sha(?:1|256|384|512)|sha2::|js_crypto_|crypto::hash_|node::crypto::Hash', stack, re.I):
        return 'hash'
    if re.search(r'miniz_oxide|flate2::|perry_ext_zlib|zng_inflate|inflate_fast|inflateBack|(?:^|[ :])inflate(?:\+|[ (]|$)|crc32|adler32|node::Zlib|node::(?:CompressionStream|ZlibStream)', stack, re.I):
        return 'inflate'
    if re.search(r'js_fs_(?:write|open|close|mkdir|chmod|rename)|perry_(?:stdlib|ext_fs)::fs::|perry_ext_fs::|node::fs::|uv_fs_(?:write|mkdir|open)|__libc_write', stack):
        return 'write / filesystem'
    tar = bool(re.search(r'tar_ts|tar.ts|LazyCompile.*(?:checksumOk|safePath|parsePax|gunzipped|createReader)', stack))
    if re.search(r'perex::|regex|regexp|fancy_regex|RegExp', stack, re.I):
        return 'tar regex (cc)' if tar else 'other regex / parseIntegrity (cc)'
    if re.search(r'object.*spread|js_object_spread|safePath', stack, re.I):
        return 'tar spreads / safePath (hwp)' if tar else 'other object work'
    if re.search(r'byte_access|typed_array|buffer::|js_buffer_|Buffer::|node::Buffer::', leaf):
        return 'tar bytes / views (hwp)' if tar else 'other bytes / clone'
    if re.search(r'promise|microtask|async_step|AsyncFunction|Promise|Microtask|closure', leaf, re.I):
        return 'promise / microtask / dispatch'
    if re.search(r'memcpy|memmove|memset|malloc|mimalloc|mi_|dealloc|free|alloc::|arena::allocators::|arena_cell_alloc', leaf):
        return 'allocation / copies'
    if re.search(r'perry_runtime::string::|js_string_', leaf):
        return 'tar header string decode / interning' if tar else 'string decode / interning'
    if re.search(r'perry_runtime::object::|js_object_|js_get_property|js_set_property', leaf):
        return 'object property lookup / dispatch'
    if re.search(r'perry_fn_pluck_ts__', leaf):
        return 'metadata JSON scanner'
    if tar:
        return 'tar parse / header decode / numbers'
    return 'other / unattributed'

results = []
for path in sorted((ROOT / 'profiles').glob('*.samples')):
    leaf_path = path.with_suffix('.leaves')
    if not leaf_path.exists():
        continue  # Discarded preliminary input-driver profiles.
    stacks = []
    for block in path.read_text().split('\n\n'):
        lines = block.splitlines()
        if not lines:
            continue
        header = re.match(r'^(.+?)\s+(\d+)\s+(\d+)\s*$', lines[0])
        if not header:
            continue
        frames = []
        for line in lines[1:]:
            match = re.match(r'\s*[0-9a-f]+ (.+) \((.+)\)', line)
            if match:
                frames.append(match[1])
        stacks.append((header[1].strip(), header[2], int(header[3]), frames))
    leaves = []
    for line in leaf_path.read_text().splitlines():
        match = re.match(r'^(.+?)\s+(\d+)\s+(\d+)\s+([0-9a-f]+)\s+(.+) \(([^)]*)\)\s*$', line)
        if match:
            leaves.append((match[1].strip(), match[2], int(match[3]), match[5]))
    assert len(stacks) == len(leaves), (path, len(stacks), len(leaves))
    counts, symbols = Counter(), Counter()
    threads = defaultdict(Counter)
    no_chain = 0
    for (comm, tid, period, frames), (lc, lt, lp, leaf) in zip(stacks, leaves):
        assert (comm, tid, period) == (lc, lt, lp), path
        category = group([leaf, *frames])
        counts[category] += period
        symbols[leaf] += period
        threads[tid][category] += period
        if not frames:
            no_chain += period
    total = sum(counts.values())
    assert total > 0
    results.append(dict(
        name=path.stem, total_sampled_period=total,
        leaf_only_pct=round(no_chain / total * 100, 3),
        percent={key: round(value / total * 100, 3) for key, value in counts.items()},
        thread_percent={tid: {key: round(value / sum(parts.values()) * 100, 3)
                             for key, value in parts.items()} for tid, parts in threads.items()},
        thread_periods={tid: sum(parts.values()) for tid, parts in threads.items()},
        top_symbols=[dict(symbol=key, percent=round(value / total * 100, 3))
                     for key, value in symbols.most_common(15)],
    ))
(ROOT / 'evidence/profiles.json').write_text(json.dumps(results, indent=2) + '\n')
for row in results:
    if 'worker' in row['name'] or 'upm' in row['name']:
        print(row['name'], row['percent'])
