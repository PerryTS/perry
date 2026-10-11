Deep recursive `try`/`finally` chains now grow jump-buffer storage in stable slabs and reach the normal catchable `RangeError` path instead of panicking at a fixed handler limit.
