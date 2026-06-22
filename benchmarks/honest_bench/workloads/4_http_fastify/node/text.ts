// honest_bench workload 4 — http_fastify_text (node).

import Fastify from 'fastify';

const app = Fastify({ logger: false });

app.get('/', async (_req, reply) => {
  reply.type('text/plain');
  return 'pong';
});

app.listen({ port: parseInt(process.env.HONEST_BENCH_HTTP_PORT || '18080', 10), host: '127.0.0.1' });
