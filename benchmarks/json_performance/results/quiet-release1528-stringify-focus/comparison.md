Nine interleaved trials per engine and fixture, each with 32,768 stringify calls and eight warmups.
Perry is the 0.5.1528 release build; prior is integrated R6 (0.5.1527); baseline is the immutable PR #10022 merge.
Node is the output oracle. All 54 timing trials passed the checksum/retention checks. Quiet admission passed before and after measurement.

| Fixture | Engine | Median CPU µs | Mean CPU µs | CPU range µs | Peak RSS MiB |
|---|---|---:|---:|---:|---:|
| long_string_1m | perry | 32.196228 | 31.654419 | 27.731476–33.715637 | 54.265625 |
| long_string_1m | baseline | 32.378937 | 32.683434 | 29.888184–35.576111 | 54.421875 |
| long_string_1m | prior | 32.574371 | 32.162106 | 27.780334–35.593109 | 54.281250 |
| unicode_1m | perry | 26.014587 | 26.365177 | 25.261322–29.208405 | 53.640625 |
| unicode_1m | baseline | 25.445770 | 25.926748 | 23.268158–29.359924 | 53.828125 |
| unicode_1m | prior | 25.804901 | 25.781643 | 23.674561–27.843414 | 53.656250 |

ASCII’s median is 0.6% below the reference; Unicode’s is 2.2% above (mean +1.7%).
The preliminary full-matrix increases of 3.9% and 3.6% did not repeat at that magnitude.
The ranges overlap, and the remaining positive Unicode difference is reported as uncertainty, not erased or claimed to be zero.
These medians are not a formal significance test. Raw trials, controller sources, binary hashes and the quiet window are preserved beside this report.
