#!/usr/bin/env python3
"""Merge translated chunk results back into po/<lang>.po deterministically.

Reads WORKDIR/out/<lang>__NNNN.json files, each shaped:
    {"name": "...", "translations": [{"i": <po_index>, "t": "<msgstr>"}, ...]}
Re-iterates each .po in file order (same order the extractor used) so the
numeric index `i` maps unambiguously back to its entry. A translation of
"__SKIP__" or "" clears the entry (English fallback) and drops the fuzzy flag.
Reports per-language applied counts and any entries still left untranslated.
"""
import json
import os

import polib

PO_DIR = os.path.join(os.path.dirname(__file__), "..", "po")
WORKDIR = "/tmp/i18n_work"
SKIP = "__SKIP__"


def load_results():
    outdir = os.path.join(WORKDIR, "out")
    by_lang = {}
    bad = []
    for fn in sorted(os.listdir(outdir)):
        if not fn.endswith(".json"):
            continue
        lang = fn.split("__")[0]
        try:
            with open(os.path.join(outdir, fn)) as fh:
                data = json.load(fh)
            trans = data["translations"] if isinstance(data, dict) else data
            for t in trans:
                by_lang.setdefault(lang, {})[int(t["i"])] = t.get("t", "")
        except Exception as e:  # noqa: BLE001
            bad.append(f"{fn}: {e}")
    return by_lang, bad


def main():
    by_lang, bad = load_results()
    if bad:
        print("MALFORMED OUTPUT FILES:")
        for b in bad:
            print("  ", b)
    for fn in sorted(os.listdir(PO_DIR)):
        if not fn.endswith(".po") or fn == "messages.pot":
            continue
        lang = fn[:-3]
        tmap = by_lang.get(lang, {})
        path = os.path.join(PO_DIR, fn)
        po = polib.pofile(path)
        applied = skipped = 0
        for idx, entry in enumerate(po):
            if idx not in tmap:
                continue
            val = tmap[idx]
            if val == SKIP or val == "":
                entry.msgstr = ""
                skipped += 1
            else:
                entry.msgstr = val
                applied += 1
            if "fuzzy" in entry.flags:
                entry.flags.remove("fuzzy")
        po.save(path)
        # remaining gaps
        rem = sum(1 for e in po if e.msgid and not e.obsolete
                  and (e.msgstr == "" or "fuzzy" in e.flags))
        print(f"{lang:10s} applied={applied:5d} skipped(code)={skipped:5d} "
              f"still_untranslated={rem:5d}")


if __name__ == "__main__":
    main()
