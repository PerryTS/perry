### Performance

- Speed up dynamic record-processing pipelines by lowering immutable aliases
  of same-module functions as direct calls and avoiding redundant dynamic
  `this` save/restore work for receiverless arrow callbacks.
