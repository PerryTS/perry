## Binary size (Linux x86_64, release profile)

Auto-optimize ON (the user path in a source checkout) = stripped output; `+sym` = `PERRY_KEEP_SYMBOLS=1`. Prebuilt = out-of-tree install linking the prebuilt full `libperry_stdlib.a` (+ ext archives); `PERRY_NO_AUTO_OPTIMIZE=1` gives byte-identical output to it in every row (see raw data).

| probe | auto before | auto after | Δ | +sym before | +sym after | Δ | prebuilt before | prebuilt after | Δ |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| hello | 14,472 | 14,472 | +0.0% | 16,320 | 16,320 | +0.0% | 14,937,936 | 14,196,952 | -5.0% |
| fetch_local | 20,402,104 | 13,548,672 | -33.6% | 31,647,696 | 23,545,680 | -25.6% | 30,867,792 | 25,628,120 | -17.0% |
| http | 18,190,824 | 12,308,128 | -32.3% | 29,031,064 | 22,056,672 | -24.0% | 30,784,240 | 25,536,376 | -17.0% |
| https | 18,236,144 | 12,357,608 | -32.2% | 29,081,216 | 22,111,144 | -24.0% | 30,821,368 | 25,573,504 | -17.0% |
| net | 13,707,232 | 9,958,488 | -27.3% | 23,644,288 | 19,368,976 | -18.1% | 28,874,040 | 23,998,208 | -16.9% |
| tls_net | 13,756,712 | 9,995,744 | -27.3% | 23,698,416 | 19,410,784 | -18.1% | 28,915,328 | 24,039,496 | -16.9% |
| ws | 10,990,536 | 9,531,320 | -13.3% | 20,466,488 | 18,729,952 | -8.5% | 28,606,928 | 23,780,416 | -16.9% |
| crypto | 11,329,896 | 10,338,160 | -8.8% | 16,595,392 | 15,384,552 | -7.3% | 28,541,288 | 23,726,888 | -16.9% |
| zlib | 11,395,224 | 9,915,536 | -13.0% | 18,822,904 | 17,066,072 | -9.3% | 28,671,928 | 23,828,728 | -16.9% |
| child | 8,895,272 | 7,923,864 | -10.9% | 13,396,080 | 12,256,016 | -8.5% | 15,204,336 | 14,500,400 | -4.6% |
| timers | 8,620,856 | 7,284,776 | -15.5% | 13,081,032 | 11,528,792 | -11.9% | 14,946,240 | 14,213,632 | -4.9% |
| worker | 9,138,016 | 7,662,480 | -16.1% | 13,857,336 | 12,110,392 | -12.6% | 28,327,608 | 23,488,560 | -17.1% |
| backend | 24,085,816 | 17,263,936 | -28.3% | 35,920,336 | 27,833,904 | -22.5% | 31,306,440 | 26,058,576 | -16.8% |
| container | 9,027,744 | 8,158,616 | -9.6% | 13,741,968 | 12,814,688 | -6.7% | 28,217,272 | – | – |
| bench_http_server | 18,137,288 | 12,279,232 | -32.3% | 28,968,992 | 22,023,744 | -24.0% | 30,738,896 | 25,499,224 | -17.0% |
| bench_fetch_client † | 28,300,336 | 10,270,736 | -63.7% | 38,057,696 | 17,002,320 | -55.3% | 28,300,336 | 23,457,200 | -17.1% |

† BEFORE's auto-optimize cargo build FAILED for this program's feature set (`async-runtime,web-fetch`: `unresolved import base64` in perry-stdlib) and perry fell back to the prebuilt full stdlib, so the BEFORE auto number equals its prebuilt number. Not a like-for-like auto row; `fetch_local` (fetch + a server) is the comparable fetch row. Fixed somewhere in the window — AFTER builds that set.

### Subject-is-live check: tokio-family symbols in the `+sym` binaries

| probe | tokio before→after | hyper | reqwest | h2 | tower | mio | tokio_rustls | turnloop | `tokio-1.x` strings (stripped) |
|---|---|---|---|---|---|---|---|---|---|
| hello | 0→0 | 0→0 | 0→0 | 0→0 | 0→0 | 0→0 | 0→0 | 0→0 | 0→0 |
| fetch_local | 1422→0 | 1012→0 | 406→0 | 956→0 | 81→0 | 58→6 | 210→0 | 818→1081 | 83→0 |
| http | 1362→0 | 1012→0 | 380→0 | 956→0 | 79→0 | 58→6 | 210→0 | 611→857 | 84→0 |
| https | 1362→0 | 1012→0 | 380→0 | 956→0 | 79→0 | 58→6 | 210→0 | 611→857 | 84→0 |
| net | 686→0 | 0→0 | 0→0 | 0→0 | 0→0 | 55→6 | 63→0 | 373→478 | 77→0 |
| tls_net | 686→0 | 0→0 | 0→0 | 0→0 | 0→0 | 55→6 | 63→0 | 373→478 | 77→0 |
| ws | 186→0 | 0→0 | 0→0 | 0→0 | 0→0 | 8→6 | 0→0 | 425→437 | 39→0 |
| crypto | 186→0 | 0→0 | 0→0 | 0→0 | 0→0 | 8→6 | 0→0 | 339→356 | 39→0 |
| zlib | 186→0 | 0→0 | 0→0 | 0→0 | 0→0 | 8→6 | 0→0 | 291→299 | 39→0 |
| child | 0→0 | 0→0 | 0→0 | 0→0 | 0→0 | 6→6 | 0→0 | 290→298 | 0→0 |
| timers | 0→0 | 0→0 | 0→0 | 0→0 | 0→0 | 6→6 | 0→0 | 290→298 | 0→0 |
| worker | 186→0 | 0→0 | 0→0 | 0→0 | 0→0 | 8→6 | 0→0 | 291→299 | 39→0 |
| backend | 1434→0 | 1008→0 | 409→0 | 956→0 | 81→0 | 58→6 | 210→0 | 860→1131 | 83→0 |
| container | 193→0 | 0→0 | 0→0 | 0→0 | 0→0 | 8→6 | 0→0 | 291→387 | 56→0 |
| bench_http_server | 1362→0 | 1012→0 | 380→0 | 956→0 | 79→0 | 58→6 | 210→0 | 611→857 | 84→0 |
| bench_fetch_client | 1221→0 | 659→0 | 364→0 | 808→0 | 77→0 | 26→6 | 123→0 | 688→559 | 52→0 |

### Size attribution by crate (symbol bytes in the `+sym` binary)

**fetch_local** — attributed symbol bytes 17,577,288 → 12,022,900 (-31.6%)

| crate | before | after | Δ bytes |
|---|---:|---:|---:|
| perry_runtime | 6,030,062 | 5,471,019 | -559,043 |
| perry exports (js_*/perry_*) | 1,825,884 | 1,655,206 | -170,678 |
| aws-lc (C) | 1,462,333 | 0 | -1,462,333 |
| other/C/unmangled | 1,206,726 | 366,367 | -840,359 |
| brotli | 827,549 | 0 | -827,549 |
| perry_ext_http | 630,424 | 538,273 | -92,151 |
| rustls | 591,967 | 496,463 | -95,504 |
| h2 | 383,695 | 0 | -383,695 |
| perry_stdlib | 348,735 | 375,952 | +27,217 |
| core | 288,319 | 277,709 | -10,610 |
| hyper | 223,441 | 0 | -223,441 |
| perry_ext_net | 221,702 | 193,113 | -28,589 |
| tokio | 221,493 | 0 | -221,493 |
| ring (C) | 211,114 | 211,114 | +0 |
| brotli_decompressor | 190,752 | 158,086 | -32,666 |
| std | 183,052 | 167,528 | -15,524 |
| hyper_util | 163,770 | 0 | -163,770 |
| turnloop | 158,766 | 159,699 | +933 |
| alloc | 155,981 | 133,178 | -22,803 |
| hashbrown | 151,417 | 150,538 | -879 |
| zstd (C) | 135,556 | 135,556 | +0 |
| ring | 109,748 | 106,801 | -2,947 |

**bench_http_server** — attributed symbol bytes 15,681,705 → 10,999,282 (-29.9%)

| crate | before | after | Δ bytes |
|---|---:|---:|---:|
| perry_runtime | 5,889,362 | 5,356,843 | -532,519 |
| perry exports (js_*/perry_*) | 1,681,545 | 1,498,244 | -183,301 |
| aws-lc (C) | 1,462,333 | 0 | -1,462,333 |
| other/C/unmangled | 1,189,573 | 357,604 | -831,969 |
| perry_ext_http | 630,424 | 538,273 | -92,151 |
| rustls | 591,500 | 495,010 | -96,490 |
| h2 | 383,695 | 0 | -383,695 |
| core | 282,477 | 270,156 | -12,321 |
| hyper | 223,441 | 0 | -223,441 |
| perry_ext_net | 221,702 | 193,113 | -28,589 |
| tokio | 214,276 | 0 | -214,276 |
| ring (C) | 211,114 | 211,114 | +0 |
| std | 180,490 | 164,397 | -16,093 |
| hyper_util | 163,787 | 0 | -163,787 |
| turnloop | 158,371 | 159,699 | +1,328 |
| perry_stdlib | 152,269 | 152,533 | +264 |
| alloc | 145,572 | 122,109 | -23,463 |
| hashbrown | 135,057 | 132,465 | -2,592 |
| ring | 106,734 | 106,801 | +67 |
| reqwest | 102,181 | 0 | -102,181 |
| der | 100,561 | 100,561 | +0 |
| mimalloc (C) | 98,697 | 98,697 | +0 |

**net** — attributed symbol bytes 12,336,223 → 9,176,785 (-25.6%)

| crate | before | after | Δ bytes |
|---|---:|---:|---:|
| perry_runtime | 5,911,445 | 5,075,053 | -836,392 |
| aws-lc (C) | 1,462,333 | 0 | -1,462,333 |
| perry exports (js_*/perry_*) | 1,400,069 | 1,191,943 | -208,126 |
| other/C/unmangled | 964,278 | 357,341 | -606,937 |
| rustls | 524,830 | 484,006 | -40,824 |
| core | 229,907 | 227,433 | -2,474 |
| perry_ext_net | 217,276 | 191,329 | -25,947 |
| ring (C) | 0 | 211,114 | +211,114 |
| std | 173,377 | 159,409 | -13,968 |
| turnloop | 158,371 | 159,699 | +1,328 |
| perry_stdlib | 149,993 | 150,377 | +384 |
| tokio | 129,308 | 0 | -129,308 |
| hashbrown | 117,805 | 115,424 | -2,381 |
| alloc | 112,966 | 106,160 | -6,806 |
| ring | 21 | 103,021 | +103,000 |
| mimalloc (C) | 98,697 | 98,697 | +0 |
| webpki | 76,490 | 76,201 | -289 |
| der | 67,994 | 67,994 | +0 |
| serde_json | 58,348 | 272 | -58,076 |
| rustc_demangle | 46,351 | 46,351 | +0 |
| gimli | 36,503 | 36,503 | +0 |
| notify | 36,316 | 36,316 | +0 |

**backend** — attributed symbol bytes 20,859,439 → 15,324,801 (-26.5%)

| crate | before | after | Δ bytes |
|---|---:|---:|---:|
| perry_runtime | 6,057,898 | 5,499,484 | -558,414 |
| perry exports (js_*/perry_*) | 1,971,356 | 1,798,102 | -173,254 |
| brotli | 1,725,725 | 898,176 | -827,549 |
| aws-lc (C) | 1,462,333 | 0 | -1,462,333 |
| other/C/unmangled | 1,221,686 | 380,021 | -841,665 |
| zstd (C) | 642,768 | 642,768 | +0 |
| perry_ext_http | 627,453 | 538,273 | -89,180 |
| rustls | 591,970 | 496,463 | -95,507 |
| perry_stdlib | 549,111 | 573,399 | +24,288 |
| h2 | 383,695 | 0 | -383,695 |
| brotli_decompressor | 358,899 | 326,233 | -32,666 |
| core | 303,981 | 294,650 | -9,331 |
| hyper | 223,413 | 0 | -223,413 |
| tokio | 222,651 | 0 | -222,651 |
| perry_ext_net | 221,699 | 193,113 | -28,586 |
| ring (C) | 211,114 | 211,114 | +0 |
| der | 185,614 | 185,614 | +0 |
| std | 184,522 | 169,834 | -14,688 |
| turnloop | 169,743 | 172,415 | +2,672 |
| hyper_util | 163,770 | 0 | -163,770 |
| alloc | 160,337 | 137,775 | -22,562 |
| hashbrown | 155,142 | 154,263 | -879 |

**ops_loop** — attributed symbol bytes 8,148,094 → 6,805,256 (-16.5%)

| crate | before | after | Δ bytes |
|---|---:|---:|---:|
| perry_runtime | 5,784,258 | 4,779,754 | -1,004,504 |
| perry exports (js_*/perry_*) | 1,143,671 | 917,756 | -225,915 |
| core | 183,985 | 179,109 | -4,876 |
| turnloop | 152,417 | 147,842 | -4,575 |
| std | 142,979 | 135,333 | -7,646 |
| other/C/unmangled | 127,669 | 126,098 | -1,571 |
| mimalloc (C) | 98,697 | 98,697 | +0 |
| hashbrown | 83,755 | 80,948 | -2,807 |
| alloc | 74,325 | 68,407 | -5,918 |
| serde_json | 58,348 | 0 | -58,348 |
| rustc_demangle | 46,074 | 46,074 | +0 |
| gimli | 36,363 | 36,363 | +0 |
| notify | 36,316 | 36,316 | +0 |
| crossbeam_channel | 26,076 | 26,076 | +0 |
| walkdir | 17,281 | 17,281 | +0 |
| ? | 16,436 | 16,141 | -295 |
| zmij | 13,902 | 0 | -13,902 |
| ryu | 13,643 | 13,643 | +0 |
| ryu_js | 13,568 | 13,568 | +0 |
| addr2line | 11,710 | 11,710 | +0 |
| miniz_oxide | 10,035 | 10,035 | +0 |
| crossbeam_utils | 8,576 | 8,576 | +0 |

## Archive sizes (Linux, release)

| archive | before | after | Δ |
|---|---:|---:|---:|
| libperry_runtime.a | 48.42 MiB | 48.33 MiB | -0.2% |
| libperry_stdlib.a | 99.97 MiB | 86.41 MiB | -13.6% |
| libperry_ext_ads.a | 23.12 MiB | 23.02 MiB | -0.4% |
| libperry_ext_argon2.a | 23.95 MiB | 23.86 MiB | -0.4% |
| libperry_ext_bcrypt.a | 23.68 MiB | 23.59 MiB | -0.4% |
| libperry_ext_better_sqlite3.a | 27.32 MiB | 27.22 MiB | -0.4% |
| libperry_ext_cheerio.a | 30.46 MiB | 30.36 MiB | -0.3% |
| libperry_ext_decimal.a | 24.12 MiB | – MiB | removed |
| libperry_ext_ethers.a | 23.29 MiB | 23.20 MiB | -0.4% |
| libperry_ext_events.a | 23.58 MiB | 23.54 MiB | -0.2% |
| libperry_ext_http.a | 95.52 MiB | 67.45 MiB | -29.4% |
| libperry_ext_ioredis.a | 61.77 MiB | – MiB | removed |
| libperry_ext_mongodb.a | 104.91 MiB | – MiB | removed |
| libperry_ext_net.a | 52.61 MiB | 39.69 MiB | -24.5% |
| libperry_ext_nodemailer.a | 30.09 MiB | 29.49 MiB | -2.0% |
| libperry_ext_parcel_watcher.a | 28.46 MiB | 28.35 MiB | -0.4% |
| libperry_ext_pdf.a | 53.23 MiB | 53.14 MiB | -0.2% |
| libperry_ext_sharp.a | 81.07 MiB | 80.98 MiB | -0.1% |
| libperry_ext_streams.a | 23.52 MiB | 23.42 MiB | -0.4% |
| libperry_ext_typescript.a | 100.76 MiB | 100.72 MiB | -0.0% |
| libperry_ext_undici.a | 24.30 MiB | 24.19 MiB | -0.4% |
| libperry_ext_ws.a | 58.82 MiB | 50.33 MiB | -14.4% |
| libperry_ext_zlib.a | 30.31 MiB | 30.22 MiB | -0.3% |
| perry (compiler binary, release, unstripped) | 85.13 MiB | 84.19 MiB | -1.1% |

### Per-program auto-optimized archives

| probe | before: runtime / stdlib / ext (MiB) | after: runtime / stdlib / ext (MiB) |
|---|---|---|
| hello | (none: tiny-program path) | (none: tiny-program path) |
| fetch_local | 35.87 / 62.25 / 92.80 | 35.25 / 48.07 / 65.11 |
| http | 35.86 / 52.57 / 92.79 | 35.25 / 44.24 / 65.10 |
| https | 35.86 / 52.57 / 92.79 | 35.25 / 44.24 / 65.10 |
| net | 35.86 / 52.55 / 52.37 | 35.25 / 44.22 / 39.55 |
| tls_net | 35.86 / 52.55 / 52.37 | 35.25 / 44.22 / 39.55 |
| ws | 35.87 / 37.99 / 48.11 | 35.25 / 36.77 / 48.42 |
| crypto | 35.89 / 42.50 / 0.00 | 35.27 / 41.27 / 0.00 |
| zlib | 35.86 / 37.98 / 0.00 | 35.24 / 36.76 / 0.00 |
| child | 35.86 / 37.98 / 0.00 | 35.24 / 36.76 / 0.00 |
| timers | 35.86 / 37.98 / 0.00 | 35.24 / 36.76 / 0.00 |
| worker | 35.86 / 37.98 / 0.00 | 35.24 / 36.76 / 0.00 |
| backend | 35.87 / 66.55 / 93.40 | 35.26 / 52.33 / 65.79 |
| container | 35.86 / 43.65 / 0.00 | 35.24 / 41.44 / 0.00 |
| bench_http_server | 35.86 / 52.57 / 92.79 | 35.25 / 44.24 / 65.10 |
| bench_fetch_client | (none: auto-opt cargo build failed, prebuilt fallback) | 35.24 / 46.72 / 0.00 |

## Compile time

`cargo build --release -j4` from a clean target dir, both arms concurrently on perrymaster (shared, load 35–75: wall is indicative only; CPU time is the comparable number).

| step | before wall | after wall | before CPU (user+sys) | after CPU | Δ CPU |
|---|---:|---:|---:|---:|---:|
| core (-p perry -p perry-runtime-static -p perry-stdlib-static) | 1,809 s | 1,760 s | 1,856 s | 1,718 s | -7.4% |
| ext (same + every governed perry-ext-* (incremental on top of core)) | 2,592 s | 2,363 s | 2,084 s | 1,849 s | -11.2% |

`perry compile` per probe (Linux, auto-optimize ON). Cold = empty object cache and the program's feature set not yet built (cargo rebuilds runtime+stdlib+ext); warm = immediate recompile.

| probe | cold rebuilt? b/a | cold CPU before | cold CPU after | Δ | warm wall before | warm wall after | Δ |
|---|---|---:|---:|---:|---:|---:|---:|
| hello | 0/0 | 0.2 s | 0.2 s | +0.0% | 0.54 s | 0.50 s | -7.4% |
| fetch_local | 1/1 | 673.0 s | 552.8 s | -17.9% | 7.53 s | 2.87 s | -61.9% |
| http | 1/1 | 649.5 s | 551.6 s | -15.1% | 2.99 s | 2.30 s | -23.1% |
| https | 0/0 | 5.1 s | 3.7 s | -26.2% | 2.93 s | 2.15 s | -26.6% |
| net | 1/1 | 551.4 s | 467.4 s | -15.2% | 3.05 s | 3.22 s | +5.6% |
| tls_net | 0/0 | 4.5 s | 4.3 s | -3.3% | 2.46 s | 4.96 s | +101.6% |
| ws | 1/1 | 521.9 s | 499.3 s | -4.3% | 5.75 s | 9.16 s | +59.3% |
| crypto | 1/1 | 488.1 s | 483.8 s | -0.9% | 1.39 s | 3.31 s | +138.1% |
| zlib | 1/1 | 381.2 s | 364.4 s | -4.4% | 1.81 s | 1.91 s | +5.5% |
| child | 1/1 | 383.8 s | 359.0 s | -6.5% | 0.88 s | 1.38 s | +56.8% |
| timers | 0/0 | 0.9 s | 1.1 s | +18.0% | 0.97 s | 1.16 s | +19.6% |
| worker | 0/0 | 1.3 s | 1.6 s | +20.0% | 1.00 s | 1.22 s | +22.0% |
| backend | 1/1 | 592.6 s | 497.1 s | -16.1% | 2.34 s | 2.09 s | -10.7% |
| container | 1/1 | 397.2 s | 388.9 s | -2.1% | 1.00 s | 1.38 s | +38.0% |
| bench_http_server | 0/0 | 4.9 s | 3.9 s | -19.5% | 2.67 s | 2.03 s | -24.0% |
| bench_fetch_client | 1/1 | 408.7 s | 485.1 s | +18.7% | 10.18 s | 2.85 s | -72.0% |

macOS arm64 (dev MacBook, shared, load varied 6→45 between arms — wall only, indicative): 

| probe | cold before | cold after | warm before | warm after | size before | size after | Δ size |
|---|---:|---:|---:|---:|---:|---:|---:|
| hello | 4 s | 4 s | 1.00 s | 0.95 s | 33,848 | 33,848 | +0.0% |
| backend | 295 s | 438 s | 1.99 s | 4.04 s | 19,063,128 | 13,419,992 | -29.6% |
| bench_http_server | 249 s | 293 s | 2.00 s | 1.99 s | 14,926,944 | 9,730,336 | -34.8% |
| bench_fetch_client | 229 s | 370 s | 3.98 s | 2.99 s | 23,710,144 | 8,138,464 | -65.7% |
| http | 249 s | 308 s | 1.98 s | 2.94 s | 14,976,616 | 9,763,432 | -34.8% |
| ws | 280 s | 295 s | 1.99 s | 2.00 s | 9,396,112 | 7,559,888 | -19.5% |
| worker | 292 s | 410 s | 2.02 s | 1.99 s | 7,923,304 | 6,169,464 | -22.1% |
| timers | 321 s | 270 s | 2.08 s | 1.99 s | 7,442,376 | 5,870,632 | -21.1% |
| crypto | 319 s | 339 s | 2.00 s | 1.98 s | 9,628,664 | 8,140,040 | -15.5% |
| zlib | 279 s | 272 s | 2.01 s | 2.00 s | 9,741,384 | 7,954,456 | -18.3% |
| net | 323 s | 385 s | 1.98 s | 3.01 s | 11,385,600 | 7,860,336 | -31.0% |
| fetch_local | failed (rc 143) | 506 s | – | 2.98 s | – | 10,706,808 | – |

## Runtime (quiet bench mini: perry-macos.fritz.box, M1, macOS; 7 interleaved rounds, 10s per load cell, median [min–max])

| metric | before | after | Δ |
|---|---:|---:|---:|
| http c=1 req/s | 20,579 [20,539–20,611] | 20,404 [20,358–20,440] | -0.9% |
| http c=1 p50 ms | 0.047 [0.047–0.047] | 0.047 [0.047–0.048] | +0.5% |
| http c=1 p99 ms | 0.057 [0.057–0.058] | 0.058 [0.057–0.059] | +2.4% |
| http c=1 CPU µs/req | 26.7 [26.6–26.8] | 27.8 [27.7–27.9] | +4.0% |
| http c=1 RSS peak KiB | 32,816 [32,800–32,832] | 25,568 [25,552–25,568] | -22.1% |
| http c=1 threads after load | 1 [1–1] | 1 [1–1] | +0.0% |
| http c=64 req/s | 99,507 [97,916–101,266] | 100,507 [96,387–102,345] | +1.0% |
| http c=64 p50 ms | 0.631 [0.622–0.644] | 0.629 [0.620–0.652] | -0.2% |
| http c=64 p99 ms | 0.872 [0.853–0.885] | 0.867 [0.835–0.897] | -0.7% |
| http c=64 CPU µs/req | 9.5 [9.4–9.6] | 9.6 [9.5–9.9] | +1.5% |
| http c=64 RSS peak KiB | 33,712 [33,680–33,760] | 28,784 [28,720–28,864] | -14.6% |
| http c=64 threads after load | 1 [1–1] | 1 [1–1] | +0.0% |
| http c=256 req/s | 107,081 [50,748–107,359] | 97,422 [45,795–97,769] | -9.0% |
| http c=256 p50 ms | 2.029 [2.024–2.442] | 2.236 [2.229–2.506] | +10.2% |
| http c=256 p99 ms | 2.323 [2.297–7.862] | 2.523 [2.507–7.776] | +8.6% |
| http c=256 CPU µs/req | 9.2 [9.2–12.6] | 10.1 [10.0–13.5] | +9.8% |
| http c=256 RSS peak KiB | 36,256 [36,080–36,336] | 31,440 [31,280–31,488] | -13.3% |
| http c=256 threads after load | 1 [1–1] | 1 [1–1] | +0.0% |
| backend c=1 req/s | 12,731 [12,721–12,787] | 12,389 [12,303–12,505] | -2.7% |
| backend c=1 p50 ms | 0.075 [0.075–0.075] | 0.078 [0.077–0.078] | +3.4% |
| backend c=1 p99 ms | 0.090 [0.090–0.091] | 0.094 [0.093–0.096] | +4.3% |
| backend c=1 CPU µs/req | 56.0 [55.7–56.1] | 58.4 [57.7–58.8] | +4.3% |
| backend c=1 RSS peak KiB | 219,440 [214,224–221,360] | 207,040 [203,632–211,360] | -5.7% |
| backend c=1 threads after load | 1 [1–1] | 1 [1–1] | +0.0% |
| http_idle RSS KiB | 14,784 [14,784–14,800] | 14,448 [14,432–14,448] | -2.3% |
| http_idle threads | 1 [1–1] | 1 [1–1] | +0.0% |
| backend_idle RSS KiB | 20,400 [20,384–20,400] | 19,952 [19,952–19,968] | -2.2% |
| backend_idle threads | 1 [1–1] | 1 [1–1] | +0.0% |
| fetch client c=1 req/s | 17,447 [13,279–18,894] | 19,049 [15,087–22,165] | +9.2% |
| fetch client c=1 CPU µs/req | 33.3 [23.3–36.7] | 30.0 [23.3–36.7] | -10.0% |
| fetch client c=1 max RSS KiB | 26,032 [26,000–26,048] | 22,944 [22,912–22,960] | -11.9% |
| fetch client c=16 req/s | 2,685 [2,678–2,701] | 64,779 [39,493–74,800] | +2312.3% |
| fetch client c=16 CPU µs/req | 61.7 [60.0–61.7] | 13.3 [11.7–15.0] | -78.4% |
| fetch client c=16 max RSS KiB | 32,368 [32,224–32,448] | 29,968 [29,632–30,160] | -7.4% |
| hello startup warm ms | 1.56 [1.35–1.59] | 1.56 [1.47–1.60] | -0.1% |
| hello startup cold (fresh copy) ms | 490.0 [150.0–570.0] | 380.0 [190.0–730.0] | -22.4% |
| hello max RSS KiB | 1,312 [1,312–1,312] | 1,312 [1,312–1,312] | +0.0% |
| backend startup warm ms | 7.48 [7.46–7.52] | 7.13 [7.09–7.15] | -4.7% |
| backend startup cold (fresh copy) ms | 340.0 [340.0–480.0] | 290.0 [290.0–440.0] | -14.7% |
| backend startup max RSS KiB | 20,560 [20,560–20,576] | 20,144 [20,128–20,144] | -2.0% |


Backend at c=8…64 in a FRESH process per cell (5 s each, 3 rounds; `mini-diag.json`) — the replacement for the invalid c=64 cells:

| conc | before req/s | after req/s | Δ | survived (before/after) |
|---|---:|---:|---:|---|
| 8 | 26,539 | 25,927 | -2.3% | 3/3 vs 3/3 |
| 16 | 30,573 | 30,528 | -0.1% | 3/3 vs 3/3 |
| 32 | 35,052 | 33,974 | -3.1% | 3/3 vs 3/3 |
| 64 | 35,128 | 34,846 | -0.8% | 3/3 vs 3/3 |

fetch-client reliability re-run (10 rounds × c=1,16 per arm): before 20/20 ok, after 20/20 ok.

Peak thread count while working (ps -M sampled every 100 ms): http_server 1, backend 1, fetch_client_c1 1, fetch_client_c16 1, crypto 1, zlib 1, worker 1, timers 1, net 1 (AFTER); BEFORE identical: http_server 1, backend 1, fetch_client_c1 1, fetch_client_c16 1, crypto 1, zlib 1, worker 1, timers 1, net 1.

Excluded samples: 18.

Host load: start [1.27, 1.17, 1.14], end [4.83, 4.31, 3.23].

### Correctness on macOS (output vs Node 26.5.1)

| probe | before | after |
|---|---|---|
| backend | match | match |
| crypto | match | match |
| hello | match | match |
| http | match | match |
| net | match | match |
| timers | match | match |
| worker | DIFF (rc=0) | DIFF (rc=0) |
| ws | DIFF (rc=timeout) | DIFF (rc=timeout) |
| zlib | match | match |

## Retired instructions (perrymaster, `perf stat -e instructions:u`, two-N differential)

per-op = (I(N2) − I(N1)) / (N2 − N1), medians over reps; fixed = I(N1) − N1·per-op. `ops_loop` is the bare-loop control (pure JS, no event loop): it should not move.

| probe | per-op before | per-op after | Δ | fixed before | fixed after | Δ |
|---|---:|---:|---:|---:|---:|---:|
| hello | – | – | – | 113,378 | 113,378 | +0.0% |
| ops_crypto | 94,586 | 78,094 | -17.4% | 1,514,133 | 1,625,796 | +7.4% |
| ops_fetch | 426,349 | 419,992 | -1.5% | 1,872,556 | 1,659,920 | -11.4% |
| ops_http ‡ | 1,578,977 | 2,220,907 | +40.7% | -53,762,481 | -108,896,072 | +102.6% |
| ops_loop | 38,000 | 38,001 | +0.0% | 792,432 | 695,848 | -12.2% |
| ops_net | 76,629 | 59,649 | -22.2% | 1,820,203 | 1,705,467 | -6.3% |
| ops_timers | 33,339 | 25,685 | -23.0% | 1,116,522 | 1,114,640 | -0.2% |
| ops_zlib | 224,545 | 203,295 | -9.5% | 1,222,350 | 1,324,484 | +8.4% |

‡ `ops_http` is NOT linear in N in either arm (I(100)/I(500)/I(1000)/I(2000) = 104/736/2,280/7,891 M before, 113/1,002/3,347 M after), so its per-op and fixed columns are not meaningful. AFTER additionally holds one socket fd per completed `http.get` round trip (301 open fds after 300 requests vs 6 before) and dies with `ECONNREFUSED` once it reaches the 1024-fd ulimit (~1,015 sequential requests); BEFORE completes N=2000. See README, *Regressions*.

## Dependency footprint

Cargo.lock packages: **1023 → 921** (-10.0%).

| crate | reachable deps before | after | Δ | async/HTTP-stack crates before | after |
|---|---:|---:|---:|---|---|
| perry | 321 | 274 | -14.6% | mio@v0.8.11 | mio@v0.8.11 |
| perry-runtime | 205 | 203 | -1.0% | mio@v0.8.11 | mio@v0.8.11 |
| perry-stdlib | 404 | 365 | -9.7% | aws-lc-rs@v1.18.1, aws-lc-sys@v0.45.0, h2@v0.4.19, hyper-rustls@v0.27.9, hyper-util@v0.1.20, hyper@v1.11.1, mio@v0.8.11, mio@v1.2.1, reqwest@v0.12.28, tokio-macros@v2.7.0, tokio-rustls@v0.26.5, tokio-util@v0.7.18, tokio@v1.53.1, tower-http@v0.6.11, tower-layer@v0.3.3, tower-service@v0.3.3, tower@v0.5.3 | mio@v0.8.11 |
| perry-ext-http | 177 | 134 | -24.3% | aws-lc-rs@v1.18.1, aws-lc-sys@v0.45.0, h2@v0.4.19, hyper-rustls@v0.27.9, hyper-util@v0.1.20, hyper@v1.11.1, mio@v1.2.1, reqwest@v0.12.28, tokio-macros@v2.7.0, tokio-rustls@v0.26.5, tokio-util@v0.7.18, tokio@v1.53.1, tower-http@v0.6.11, tower-layer@v0.3.3, tower-service@v0.3.3, tower@v0.5.3 | – |
| perry-ext-net | 48 | 33 | -31.2% | aws-lc-rs@v1.18.1, aws-lc-sys@v0.45.0, mio@v1.2.1, tokio-macros@v2.7.0, tokio-rustls@v0.26.5, tokio@v1.53.1 | – |
| perry-ext-ws | 95 | 94 | -1.1% | – | – |
| perry-ext-zlib | 25 | 25 | +0.0% | – | – |

Removed from the lockfile: aws-lc-rs, aws-lc-sys, h2, hyper, hyper-rustls, hyper-util, quinn, reqwest, tokio, tokio-macros, tokio-rustls, tokio-util, tower, tower-http, tower-layer, tower-service.

