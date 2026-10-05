Fetch response body bytes move into their body stream or consuming body method
instead of being cloned and retained. Consumption uses the reader protocol so
tee branches still deliver data after a body transfer. Buffered bodies remain
byte streams, preserving clone byte isolation; reader locks and disturbance
remain authoritative for bodyUsed, clone and consumption.

HTTP streaming delivery avoids its duplicate scratch vector and releases
completed content-decoding stages. Native capacity tests and Node parity tests
cover released storage, body transfers, cloning and locked or used bodies.
