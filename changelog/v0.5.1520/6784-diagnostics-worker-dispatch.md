### Fixed

- Make AOT static property and method dispatch IDs safe across `perry/thread`
  workers, allowing worker-side `diagnostics_channel` publishes and subscriber
  ownership checks to reach the main-thread runtime correctly.
