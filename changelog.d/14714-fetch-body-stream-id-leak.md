A long-running program that repeats keep-alive `fetch()` + `text()` (or `json()`
and similar) on small responses no longer leaks a Web Streams id per call
(#12862). Before this fix it panicked with "Web Streams handle id band
exhausted" after about 260k requests.
