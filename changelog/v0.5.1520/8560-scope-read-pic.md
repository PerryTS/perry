Fixed property-read inline caches so registering an accessor no longer disables
cache warming for every unrelated plain object in the process. Descriptor-bearing
receivers remain excluded by their object-local flag and ShapeId transition, while
a 10,000-entity ECS accumulation benchmark improves from 2.0912 ms/op to
0.3224 ms/op (6.49x faster).
