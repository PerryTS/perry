### Repo: restore CLAUDE.md, emptied by a truncating edit during four merges

`CLAUDE.md` was 0 bytes on `main` from v0.5.1288 through v0.5.1291. The cause
was a version-bump edit written as:

```python
open("CLAUDE.md", "w").write(open("CLAUDE.md").read().replace(old, new))
```

Python evaluates the `"w"` open *before* the argument expression, so the file
is truncated to zero length first and the inner `.read()` then returns an empty
string — the write stores `""`. The bump appeared to succeed and `git commit`
recorded a deletion of all 254 lines with no error anywhere.

Restored verbatim from `40214c50f` (the last commit with intact content) with
only the `**Current Version:**` line advanced to the current 0.5.1291. No other
edit; the file is byte-identical to its last good state otherwise.

The safe form, used elsewhere in the same session, reads into a variable first:

```python
c = open(p).read()
open(p, "w").write(c.replace(old, new))
```
