from collections import Counter
from pathlib import Path
import hashlib
import json

w = Path(__file__).resolve().parent
bench = w.parents[1]
manifest_path = bench / 'results/fixtures.json'
fixtures = json.loads(manifest_path.read_text())

def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, list):
        for item in value:
            yield from strings(item)
    elif isinstance(value, dict):
        for item in value.values():
            yield from strings(item)

rows = []
for fixture in fixtures:
    source = bench / '.work/fixtures' / (fixture['name'] + '.json')
    raw = source.read_bytes()
    assert hashlib.sha256(raw).hexdigest() == fixture['sha256']
    counts = Counter(strings(json.loads(raw)))
    inline, heap = Counter(), Counter()
    for value, count in counts.items():
        encoded = value.encode('utf-8', errors='surrogatepass')
        has_lone = any(0xD800 <= ord(c) <= 0xDFFF for c in value)
        (inline if len(encoded) <= 5 and not has_lone else heap)[value] = count
    duplicates = [(value, count) for value, count in heap.items() if count > 1]
    short_heap = {value: count for value, count in heap.items()
                  if len(value.encode('utf-8', errors='surrogatepass')) <= 32}
    rows.append(dict(
        fixture=fixture['name'], sha256=fixture['sha256'],
        value_strings=sum(counts.values()),
        inline_sso_strings=sum(inline.values()),
        heap_eligible_strings=sum(heap.values()),
        unique_heap_values=len(heap),
        repeated_heap_occurrences=sum(count - 1 for _, count in duplicates),
        short_heap_strings=sum(short_heap.values()),
        repeated_short_heap_occurrences=sum(count - 1 for count in short_heap.values()),
        repeated_heap_payload_bytes=sum((count - 1) * len(value.encode('utf-8', errors='surrogatepass'))
                                        for value, count in duplicates),
        repeated_heap_examples=[dict(value=value[:80], count=count)
                                for value, count in sorted(duplicates, key=lambda pair: -pair[1])[:8]],
    ))
    print(fixture['name'], 'strings', sum(counts.values()), 'SSO', sum(inline.values()),
          'heap eligible', sum(heap.values()), 'duplicate heap', rows[-1]['repeated_heap_occurrences'])

result = dict(
    source_commit=json.loads((w / 'base.json').read_text())['source_commit'],
    fixture_manifest_sha256=hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
    method='Decoded value strings only, excluding property names. DirectParser SSO admission is <=5 encoded bytes without lone surrogates.',
    limits='Source census, not measured allocations. DirectParser and the generic tape materializer both use the same short-string admission (borrowed and owned branches audited). Existing across-parse source reuse may reduce allocation further. No CPU/RSS prediction.',
    fixtures=rows,
)
(w / 'value-string-census.json').write_text(json.dumps(result, indent=2) + '\n')
assert len(rows) == 19
