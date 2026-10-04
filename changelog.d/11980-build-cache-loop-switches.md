Fixed the build cache serving an object compiled under a different
`PERRY_REGION_ELEMENTS`, `PERRY_REGION_JOINT` or `PERRY_NUMBER_LOCAL_LOOP`
setting. Each switch changes the emitted loop code, and they are now build-cache
inputs.
