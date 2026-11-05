Remove the whole-function typed-body clone (`{public}$tbody` behind a
`{public}$generic` dispatcher). It compiled a second, parameter-specialised copy
of a closure and installed an entry-check dispatcher in front of it on a static
count of runtime calls. Measured on the Zod, Effect, Fastify and TypeScript
witnesses it saved no material program work, cost Fastify about 1.8% of its
instructions through a String-proven clone, and added about 2 MB of `.text` and
a second lowering per planned closure. The planning, dispatcher, install
criterion, `PERRY_TYPED_BODIES` and `PERRY_TYPED_BODIES_LOG` knobs, the
inferred closure-receiver-parameter analysis that only fed it, and its tests are
deleted. The typed ABI (`$typed_*`), full-body specialisation (`$spec_*`) and
region mechanisms are unchanged.
