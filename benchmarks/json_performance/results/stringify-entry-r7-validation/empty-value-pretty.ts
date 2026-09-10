console.log('parsed-pretty', JSON.stringify(JSON.parse('[]'), null, 2));
console.log('literal-pretty', JSON.stringify([], null, 2));
console.log('parsed-plain', JSON.stringify(JSON.parse('[]')));
