### Internal

- **Keeps `ic_miss.rs` and `array/tests.rs` under the 2000-line file gate.**
  #9302 and #9307 each landed on a file already within ~35 lines of the cap.
  The C3C PIC test module and the `Array.prototype` method-discriminator tests
  move to sibling files; no behaviour change.

- **Drops the dead catch-all left behind the combined accumulator arm.** #9303
  added a combined `_ =>` arm without removing the `_ => false` it supersedes,
  which is an unreachable pattern and so a `-D warnings` failure. This is the
  deletion #9308 identified.
