const rows = [{id: 1, name: 'one'}, {id: 2, name: 'two'}];
console.log(JSON.stringify(rows));
console.log(JSON.stringify({x: 3}, ['x'], 2));
console.log(JSON.stringify({x: 4}, (key, value) => key === 'x' ? value + 1 : value));
console.log('%j', {ok: true});
console.log(JSON.isRawJSON(rows));
