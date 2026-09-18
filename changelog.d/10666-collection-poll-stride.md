### Faster

- A global regular-expression replace spends 1.4-3.5% fewer instructions. The loop that collects the matches asked the collector whether it was due to run once per match, which costs about 436 instructions and could never do anything there — that loop writes its matches into a native buffer and creates nothing the collector can free. It now asks once per 64 matches, which is the same bound each search was already keeping. Peak memory is unchanged, measured over nine interleaved rounds of a replace at n=1,000,000 (#10165).
