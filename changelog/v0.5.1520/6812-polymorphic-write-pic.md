## Bounded polymorphic object-write PIC

Static object writes now retain a second shape/slot cache entry. Stable
two-shape receiver sites can stay on the guarded direct-store path while
exotic, mutable, and higher-polymorphism cases continue through the complete
runtime miss path.
