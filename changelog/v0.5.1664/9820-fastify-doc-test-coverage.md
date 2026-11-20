Restore the three Fastify documentation examples to the host doc-test CI run.
The examples declare `requires: auto-optimize` in their banners, which lets the
harness rebuild their specialized runtime libraries while ordinary examples
continue using prebuilt archives. Required examples participate in the normal
pass/fail report; compiler failures remain gate failures. Their existing
compile-only setting avoids starting servers or connecting to external services.
