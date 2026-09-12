### Performance

- **DataView numeric setters avoid a transient GC handle and view-table probe
  when their offset and value are already Numbers.** DataView construction now
  caches the stable byte pointer into its canonical backing allocation, while
  the existing view registry remains the traced owning edge. Calls that can
  coerce user values or write BigInts retain their handle scope and reload the
  receiver after callbacks, including a moving collection or detach.

  On the issue's seeded set-and-read workload, serialized release measurements
  improve Perry time by 9.0–10.2% across the stable 100–100,000-element range.
  The known threshold anomaly limits the one-million-element run to 0.8%, while
  its setter/getter ratio is still 1.90x instead of the issue's original 6.68x.
  All eight numeric kinds, both byte orders, shared-view coherency,
  coercion/error ordering, detach, and forced evacuation are covered by Node
  parity tests.
