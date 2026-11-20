Program string literals are valid string operands again. Object model step 4
gave every executable string image an 8-aligned atom-hash trailer and grew its
`GcHeader::size` to cover it, but the image classifier still required the old
size, so every literal failed string validation: `path.join(".next",
"BUILD_ID")` returned "." and `path.normalize("a/b")` threw
ERR_INVALID_ARG_TYPE. The Next.js standalone server read the `.next` directory
as its BUILD_ID and exited with EISDIR. One `image_string_gc_size` now defines
the layout for the classifier and the runtime's own images, and the classifier
checks the whole extent, trailer included, against the section bounds.
