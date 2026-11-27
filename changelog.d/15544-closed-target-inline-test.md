- **`closed_target_inline` test follows the frame-silent admission rule.**
  The integration test now expects a facts-closed target that allocates
  (`Shape.describe`) to keep its call, and checks `Shape.scaled`'s signature
  entry, the body its closed site actually inlines.
