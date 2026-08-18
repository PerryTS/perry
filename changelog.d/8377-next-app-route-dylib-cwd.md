The Next App Route dylib gate now builds its provider runtime from the
repository root, so Cargo discovers Perry's required unwind-table configuration
even when the gate is invoked from outside the checkout.
