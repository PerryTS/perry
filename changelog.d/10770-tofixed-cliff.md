**`toFixed(7)` no longer costs 10.7× `toFixed(6)`.**

`spec_to_fixed` asked `format!("{x:.1100}")` on every input — 1100 being the smallest subnormal's worst case — so `(6.0).toFixed(7)` expanded 1100 decimal places through dragon4 and discarded 1093. And `POW10` was seven entries local to `fmt_fixed_int` while the admission bound read `dp <= 6` a hundred lines away, as though it were an overflow limit rather than a table length.

dp 7 goes **7125.4 → 633.1** and dp 8 **7159.1 → 645.6**, turning a 7.5× loss against node and bun into a win. A money-shaped `(12.34).toFixed(8)` goes **12637.2 → 596.6, 21.2×**. dp 0–6 gain 4–5% as well, because `10u64.pow(dp)` becomes a table load.

The table is now `POW10_FIXED` at module scope with 20 entries, and its length *is* the admission bound rather than a second constant that happens to agree.
