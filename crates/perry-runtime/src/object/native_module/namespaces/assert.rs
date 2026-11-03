//! Item-level native namespace declarations for `assert` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_ASSERT_STRICT = "assert/strict" {
        cell = "node:assert.strict";
        members = {
            Assert = (ref("node:assert.Assert"));
            AssertionError = (ref("node:assert.AssertionError"));
            ok = (ref("node:assert/strict.default"));
            fail = (ref("node:assert/strict.fail"));
            equal = (ref("node:assert/strict.equal"));
            notEqual = (ref("node:assert/strict.notEqual"));
            deepEqual = (ref("node:assert/strict.deepEqual"));
            notDeepEqual = (ref("node:assert/strict.notDeepEqual"));
            deepStrictEqual = (ref("node:assert/strict.deepStrictEqual"));
            notDeepStrictEqual = (ref("node:assert/strict.notDeepStrictEqual"));
            strictEqual = (ref("node:assert/strict.strictEqual"));
            notStrictEqual = (ref("node:assert/strict.notStrictEqual"));
            partialDeepStrictEqual = (ref("node:assert/strict.partialDeepStrictEqual"));
            match = (ref("node:assert/strict.match"));
            doesNotMatch = (ref("node:assert/strict.doesNotMatch"));
            throws = (ref("node:assert/strict.throws"));
            rejects = (ref("node:assert/strict.rejects"));
            doesNotThrow = (ref("node:assert/strict.doesNotThrow"));
            doesNotReject = (ref("node:assert/strict.doesNotReject"));
            ifError = (ref("node:assert/strict.ifError"));
            strict = (ref("node:assert.strict"));
        }
    }
}
