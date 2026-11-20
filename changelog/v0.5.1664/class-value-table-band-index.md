Fix a 128 MiB RSS jump on the first property read of an fs.Stats object
(`stats.mtime` through an untyped helper: RssAnon 2 MB -> 132 MB). The
per-agent class-value table, which holds each class function object and with
it the class prototype link, was a two-level page table indexed by the raw
class id; the first native class with a reserved id (fs.Stats, `0xFFFF_0070`)
grew its page directory to 16M zero-filled entries. Each class-id band
(compiled, runtime builtin, synthetic, reserved native) now has its own
directory indexed by the id's offset within the band, so no id can size a
directory past 256 pages.
