// honest_bench workload 4 — http_fastify_minimal (perry).
//
// Minimal Fastify GET route returning a fixed JSON body. Isolates
// the framework overhead — JSON.stringify of {ok: true} only.
// Bench client (oha) drives -c 10 sustained load via
// harness/run_http_bench.sh; this process keeps listening until
// SIGTERM'd by the harness.

import Fastify from 'fastify';

const app = Fastify({ logger: false });

app.get('/', async () => ({ ok: true }));

app.listen({ port: parseInt(process.env.HONEST_BENCH_HTTP_PORT || '18080', 10), host: '127.0.0.1' });
