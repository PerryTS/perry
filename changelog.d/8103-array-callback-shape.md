### Performance

- Propagate proven local-array element shapes into direct inline arrow
  callbacks for `forEach`, `map`, `reduce`, and `reduceRight`. Field reads on a
  contained callback element now use fixed-offset loads instead of repeating
  the class-shape and by-name lookup guards. Opaque, escaping, asynchronous,
  generator, recursive, rest/`arguments`, and source-array-aliasing callbacks
  remain on the guarded path.
