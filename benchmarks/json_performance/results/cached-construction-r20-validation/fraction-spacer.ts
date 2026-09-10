for (const space of [0, -0, 0.1, 0.5, 0.99, 1, 1.5, -0.5]) console.log(space, JSON.stringify(JSON.stringify({a: 1}, null, space)));
