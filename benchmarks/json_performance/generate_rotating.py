#!/usr/bin/env python3
"""Eight equal-sized input variants per fixture, generated outside timing."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def vary(value, variant):
    """Change one value while preserving the shape and encoded byte count."""
    if isinstance(value, dict):
        for key, child in value.items():
            changed, replacement = vary(child, variant)
            if changed:
                value[key] = replacement
                return True, value
    elif isinstance(value, list):
        for index, child in enumerate(value):
            changed, replacement = vary(child, variant)
            if changed:
                value[index] = replacement
                return True, value
    elif isinstance(value, (int, float)) and not isinstance(value, bool):
        replacement = value + variant
        if len(json.dumps(replacement)) == len(json.dumps(value)):
            return True, replacement
    elif isinstance(value, str) and value and value[0].isascii() and value[0].isalpha():
        return True, chr(ord('a') + variant) + value[1:]
    return False, value


def main():
    fixtures = json.loads((ROOT / 'results/fixtures.json').read_text())
    destination = ROOT / '.work/rotating'
    destination.mkdir(parents=True, exist_ok=True)
    manifest = []
    for fixture in fixtures:
        original = (ROOT / '.work/fixtures' / (fixture['name'] + '.json')).read_bytes()
        hashes = []
        for variant in range(8):
            _, value = vary(json.loads(original), variant)
            blob = json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode()
            if len(blob) != len(original):
                raise ValueError(f"Size changed: {fixture['name']} variant {variant}")
            (destination / f"{fixture['name']}.{variant}.json").write_bytes(blob)
            hashes.append(hashlib.sha256(blob).hexdigest())
        # null and {} have no values to vary. Separate file reads still create
        # separate heap input strings; neither fixture can hit the parse caches.
        expected_distinct = 1 if fixture['name'] in {'null', 'empty_object'} else 8
        if len(set(hashes)) != expected_distinct:
            raise ValueError(f"Variant coverage failed: {fixture['name']}")
        manifest.append(dict(fixture=fixture['name'], bytes=len(original), sha256=hashes,
                             distinct_contents=len(set(hashes))))
    (destination / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'Generated {len(manifest)} eight-input corpora with verified sizes and diversity')


if __name__ == '__main__':
    main()
