Fix a silent security downgrade in `node:crypto` scrypt: the callback form now
forwards `N`/`cost`, `r`/`blockSize`, `p`/`parallelization`, and `maxmem`
instead of always deriving with Node's defaults. Both callback and synchronous
forms now reject invalid combinations and insufficient `maxmem` with a
Node-compatible `RangeError`, rather than silently substituting the weaker
default work factor.
