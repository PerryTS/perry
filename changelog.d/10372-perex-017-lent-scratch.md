### Faster

- `RegExp` calls no longer build their match scratch per call. A `.test()` on a short string costs about a third fewer instructions, and `exec` about a quarter fewer, because the thread lends one scratch cell to each search instead of constructing and moving a fresh one (#10166).

### Changed

- The regex engine moves from perex 0.1.4 to 0.1.7. The `v` flag is complete: string members (`[\q{abc|de}]`), the properties of strings such as `\p{RGI_Emoji}`, and set operators such as `[a--b]` all compile and match. A start-anchored pattern now tries only one start.
