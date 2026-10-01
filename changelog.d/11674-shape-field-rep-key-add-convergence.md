When a property is sometimes added with a non-number value, objects built the
same way now settle on one shape for it instead of splitting between a
number-only shape and a general one, so code that reads or writes that
property keeps one fast path.
