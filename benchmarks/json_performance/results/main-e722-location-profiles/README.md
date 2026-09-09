# Merged-main stack locations

Two-second samples of freshly built main e7223f700's changing-input parser,
using eight preloaded sources. Workers intentionally terminate after sampling;
no completed throughput or RSS measurements are inferred from these samples.
The first attempt was refused before launch at load4.49; the saved window is
the later successful attempt. No other process was stopped or modified.

The Unicode fixture is an object containing a dominant long string. Its 1522
main-thread samples include 607 at UTF-16 counting, 353 at nesting preflight,
322 at token scanning, and 158 at copying. These locations still support
removing redundant scans after the escaped-record correction is validated.

The small-record sample includes 353 loader/startup samples; it is not a clean
whole-run percentage profile. The separate 1139-sample main-loop stack contains
generic object parsing, pressure checks and arena occupancy queries, template
bookkeeping, and string construction. Treat it as location evidence only.
