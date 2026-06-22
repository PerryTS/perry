// honest_bench workload 4 — http_fastify_minimal (node).
// Same surface as perry/minimal.ts, run via
// `node --import tsx node/minimal.ts` against the upstream npm fastify.

import Fastify from 'fastify';

const app = Fastify({ logger: false });

app.get('/', async () => ({ ok: true }));

app.listen({ port: parseInt(process.env.HONEST_BENCH_HTTP_PORT || '18080', 10), host: '127.0.0.1' });
