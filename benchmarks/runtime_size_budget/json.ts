const cfg = JSON.parse('{"name":"app","port":8080,"tags":["a","b"],"nested":{"x":1.5}}');
cfg.port += 1;
console.log(JSON.stringify(cfg));
console.log(JSON.stringify({ ok: true, list: [1, 2, 3] }, null, 2));
