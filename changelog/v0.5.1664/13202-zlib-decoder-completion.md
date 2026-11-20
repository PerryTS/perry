Fix zlib decoder completion: honor explicit trailing-garbage rejection and permissive `finishFlush` for truncated input while preserving corruption and dictionary errors. Add parity coverage for direct and captured synchronous calls, gzip members and padding, and fixed compressed prefixes.

Align gunzipSync and inflateRawSync API manifest metadata with their optional decoder options.
