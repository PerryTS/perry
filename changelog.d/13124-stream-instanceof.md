Link Web Stream subclass prototypes to their native bases and use the live prototype chain for instanceof, so class X extends TransformStream instances satisfy both instanceof X and instanceof TransformStream.

Class parent metadata is registered once by the defining module. Importers retain class lookup information without repeating parent registration, so importing a native subclass does not perturb unrelated function partitioning and inlining.
