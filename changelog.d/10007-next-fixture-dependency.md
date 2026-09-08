Update the Next App Route release fixture to Next 16.3.3 with matching SWC and
environment packages. Derive fixture version receipts from the package pin and
installed version, and fail if they differ, instead of reporting a stale literal.

Repair the separate shared-provider fixture workspace by removing the obsolete
`bundled-slugify` feature and refreshing its lockfile for current Perry path
dependencies. Newly required registry packages use versions already locked by
the main workspace.
