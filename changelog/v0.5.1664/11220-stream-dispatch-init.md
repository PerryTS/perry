Initialize Web Streams dispatch before running programs that use global
CompressionStream, DecompressionStream, TextEncoderStream, or TextDecoderStream
constructors (#11125). Detect these stdlib dependencies before entry codegen,
including captured, aliased, and class-body uses, so endpoint properties and
reader/writer calls work after static type information is lost.

Add compiler feature-detection coverage and a standalone native regression that
does not accidentally enable stream dispatch through an unrelated stream API.
