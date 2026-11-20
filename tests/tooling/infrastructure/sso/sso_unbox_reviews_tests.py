"""Positive and fail-closed controls for exact SSO finding reviews."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile


SOURCE = '''
fn heap_alias(bits: u64) {
    if bits & TAG_MASK == STRING_TAG {
        js_string_addref((bits & POINTER_MASK) as *mut StringHeader);
    }
}
fn clean_identity(bits: u64) -> u64 { bits }
'''


def check_reviews(inventory, expect):
    with tempfile.TemporaryDirectory(prefix="MCI-sso-review-") as temp:
        root = Path(temp)
        rel = "crates/perry-runtime/src/lib.rs"
        source = root / rel
        source.parent.mkdir(parents=True)
        source.write_text(SOURCE)
        caller = root / "caller.rs"
        caller.write_text("// SSO callers retain their ordinary path.\n")
        path = root / "reviews.json"

        def digest(text, name):
            code = inventory.mask_non_code(text)
            start, body = next((start, body) for fn, start, body, _ in inventory.functions(code)
                               if fn == name)
            return hashlib.sha256(text[start:start + len(body)].encode()).hexdigest()

        rows = [{"path": rel, "function": "heap_alias", "rule": rule,
                 "body_sha256": digest(SOURCE, "heap_alias"),
                 "claim": "Only heap alias refcounts need demotion; inline values have no header.",
                 "support_files": ["caller.rs"]}
                for rule in ("mask-cast", "heap-tag-only")]
        document = {"schema_version": 1, "reviews": rows,
                    "support_files": {"caller.rs": hashlib.sha256(caller.read_bytes()).hexdigest()}}

        def run(doc=None, text=SOURCE):
            source.write_text(text)
            path.write_text(json.dumps(document if doc is None else doc))
            findings = inventory.scan_mask_casts("perry-runtime", rel, text)
            return inventory.apply_reviews(root, findings, path)

        remaining, errors, reviewed = run()
        expect(not remaining and not errors and reviewed == 2,
               f"exact reviewed operations must pass: {(remaining, errors, reviewed)}")
        changed = SOURCE.replace("== STRING_TAG", "!= STRING_TAG")
        remaining, errors, reviewed = run(text=changed)
        expect(bool(errors) and reviewed == 0 and len(remaining) == 2,
               "changed heap guard must invalidate both reviews without suppressing findings")
        _, errors, reviewed = run(text=SOURCE.replace("heap_alias", "removed_alias"))
        expect(bool(errors) and reviewed == 0, "missing function must invalidate its reviews")
        unmatched = copy.deepcopy(document)
        unmatched["reviews"][0].update(function="clean_identity",
                                       body_sha256=digest(SOURCE, "clean_identity"))
        _, errors, reviewed = run(unmatched)
        expect(bool(errors) and reviewed == 0, "a review without a live finding must fail")
        duplicate = copy.deepcopy(document)
        duplicate["reviews"].append(copy.deepcopy(rows[0]))
        _, errors, _ = run(duplicate)
        expect(bool(errors), "duplicate finding reviews must fail")
        caller.write_text("// Fallback changed.\n")
        _, errors, reviewed = run()
        expect(bool(errors) and reviewed == 0, "changed SSO fallback source must fail")
        caller.unlink()
        _, errors, reviewed = run()
        expect(bool(errors) and reviewed == 0, "missing SSO fallback source must fail")
        caller.write_text("// SSO callers retain their ordinary path.\n")
        stale = copy.deepcopy(document)
        stale["support_files"]["unused.rs"] = "0" * 64
        _, errors, reviewed = run(stale)
        expect(bool(errors) and reviewed == 0, "unmatched source bindings must fail")
        remaining, errors, reviewed = run(text=SOURCE + inventory.PLANTED_RUNTIME)
        expect(not errors and reviewed == 2 and len(remaining) == 4,
               "unreviewed new findings must remain gated alongside valid reviews")
        path.unlink()
        _, errors, reviewed = inventory.apply_reviews(root, [], path)
        expect(bool(errors) and reviewed == 0, "a missing review registry must fail closed")
