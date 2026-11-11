Removes two workspace crates that no code consumes. `perry-db-turnloop` was
the loop-driven transport the four turnloop database bindings shared, and
`perry-tls-turnloop` was the public TLS installer for a turnloop socket that
only `perry-db-turnloop` consumed; the database bindings themselves were
replaced by pure-TypeScript drivers, so both crates were dead weight. The
reserved database slot band 9..=12 stays reserved, and the subsystem-slot gate
(`scripts/subsystem_slots.py`) now resolves every slot number from each
binding's own crate instead of a shared ledger.

- **Deleted**: `crates/perry-db-turnloop`, `crates/perry-tls-turnloop`, and
  their `workspace.dependencies` / workspace-member entries.
