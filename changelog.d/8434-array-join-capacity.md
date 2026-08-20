### Performance

- Preallocate `Array.prototype.join`'s exact separator-byte floor instead of
  growing its Rust assembly buffer from zero capacity. Element coercion remains
  single-pass, so user `toString` side effects are not duplicated.
