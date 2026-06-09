#!/usr/bin/env python3
"""Extract untranslated + fuzzy .po entries into JSON chunk files for translation.

For each po/<lang>.po, iterates entries in file order, selects those that are
untranslated (empty msgstr) or fuzzy, and writes fixed-size chunk files to
WORKDIR/in/<lang>__<NNNN>.json. Each entry is keyed by a stable per-language
sequential index `i` so results can be merged back deterministically.
"""
import json
import os
import sys

import polib

PO_DIR = os.path.join(os.path.dirname(__file__), "..", "po")
WORKDIR = "/tmp/i18n_work"
MAX_ENTRIES = 170
MAX_CHARS = 7000


def page_of(entry):
    if entry.occurrences:
        f = entry.occurrences[0][0]
        return f.replace("src/", "")
    return ""


def main():
    indir = os.path.join(WORKDIR, "in")
    os.makedirs(indir, exist_ok=True)
    manifest = {}
    for fn in sorted(os.listdir(PO_DIR)):
        if not fn.endswith(".po") or fn == "messages.pot":
            continue
        lang = fn[:-3]
        po = polib.pofile(os.path.join(PO_DIR, fn))
        todo = []
        idx = 0
        for entry in po:
            if entry.obsolete:
                idx += 1
                continue
            if entry.msgid == "":  # header
                idx += 1
                continue
            is_fuzzy = "fuzzy" in entry.flags
            untranslated = entry.msgstr == ""
            if untranslated or is_fuzzy:
                todo.append({"i": idx, "page": page_of(entry), "text": entry.msgid})
            idx += 1
        # chunk
        chunks = []
        cur, cur_chars = [], 0
        for e in todo:
            t = len(e["text"])
            if cur and (len(cur) >= MAX_ENTRIES or cur_chars + t > MAX_CHARS):
                chunks.append(cur)
                cur, cur_chars = [], 0
            cur.append(e)
            cur_chars += t
        if cur:
            chunks.append(cur)
        for n, ch in enumerate(chunks):
            name = f"{lang}__{n:04d}"
            with open(os.path.join(indir, name + ".json"), "w") as fh:
                json.dump({"lang": lang, "name": name, "entries": ch}, fh, ensure_ascii=False)
        manifest[lang] = {"total_entries": len(po), "todo": len(todo), "chunks": len(chunks)}
        print(f"{lang}: {len(todo)} todo -> {len(chunks)} chunks")
    with open(os.path.join(WORKDIR, "manifest.json"), "w") as fh:
        json.dump(manifest, fh, indent=2)
    total_chunks = sum(m["chunks"] for m in manifest.values())
    total_todo = sum(m["todo"] for m in manifest.values())
    print(f"\nTOTAL: {total_todo} entries, {total_chunks} chunks")


if __name__ == "__main__":
    main()
