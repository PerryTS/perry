// HWS node: bun_file_text.node.mjs
Bun.file("value.json").text().then(text => console.log(text.trim()));
