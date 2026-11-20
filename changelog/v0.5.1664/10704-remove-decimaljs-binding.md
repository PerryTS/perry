**Removed the native `decimal.js` binding** (and `big.js`/`bignumber.js`, which shared the same
crate and defects) — `import Decimal from "decimal.js"` now resolves to the real npm package,
compiled from source. Native division silently returned `"1"` for both `1/3` and `10/4`, and
`new Decimal("123456789123456789").times("987654321987654321")` aborted the process
(`Multiplication overflowed` in `rust_decimal`, a fixed 96-bit type backing an arbitrary-precision
library); `instanceof`/`constructor.name` were also broken. Fixes #10684. Requires #10439's
import-provenance fix (#10699) to reach the real package at its default import name.
