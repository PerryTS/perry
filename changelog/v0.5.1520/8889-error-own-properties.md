Fixed an fs error's `code`/`errno`/`syscall`/`path`: they are now own properties
of the **error object** rather than entries in side tables keyed by the error's
**message string address**.

Two defects followed from that keying, both verified against node.

**The wrong error got the metadata.** Any `new Error(m)` built from the same
message text inherited an unrelated fs error's fields — `.code` returned
`ENOENT` where node returns `undefined`, along with `.syscall`, `.errno` and
`.path`. The metadata belonged to the string, so anything holding that string
answered to it.

**They were invisible to reflection.** In node these are ordinary own
properties; served from a side table behind property *getters* they appeared in
none of the enumeration paths:

| | before | after (= node) |
|---|---|---|
| `Object.keys(e)` | `[]` | `code,errno,path,syscall` |
| `hasOwnProperty('code')` | `false` | `true` |
| `getOwnPropertyDescriptor` | `undefined` | `{value,writable,enumerable,configurable}` |
| `JSON.stringify(e)` | `{}` | `{"errno":-2,"code":"ENOENT",…}` |
| `{...e}` | `{}` | same |

Any code that logged or serialised a caught fs error lost its whole payload.

Three sites each held a different wrong assumption about errors. The fs builders
keyed on the message string. `JSON.stringify` hardcoded `"{}"` for
`GC_TYPE_ERROR` — correct for a *plain* error, whose `message`/`name`/`stack`
are non-enumerable, but wrong once an error carries enumerable own properties,
so it also dropped **user-assigned** ones (`e.foo=1; JSON.stringify(e)` gave
`{}` where node gives `{"foo":1}`; that half is independent of fs).
`Object.assign`/spread had no Error arm and copied nothing. All three now
enumerate through `exotic_own_keys(.., enumerable_only = true)` — the same
enumeration `Object.keys` uses — so they cannot drift apart again.

Property **order** is fixed too. `ERROR_USER_PROPS` was a `HashMap` with an
alphabetical `sort_by` bolted on for determinism: stable, but not node's. Own
string keys enumerate in insertion order per ECMA-262, and that order is
observable through all four paths above. The store is insertion-ordered now,
reassignment keeps a key's original position (`o.a=1; o.b=2; o.a=3` enumerates
`a,b`), and the fs fields install in node's `uvException` order. The GC root
scanner over these properties moved to the ordered store.

Verified by running three repro programs against node on the same host:
byte-identical output, key order included.
