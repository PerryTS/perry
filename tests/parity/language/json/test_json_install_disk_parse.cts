const filename = __dirname + '/fixtures/json_install_data.json';
const data = require(filename);
console.log(data.answer, data.items.length, data.items[2]);
