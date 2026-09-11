function identity(key: string, value: any): any { return value; }
const values: string[] = ['\ud800', '\udc00', 'ordinary prefix then \ud800', '\ud800\udc00'];
for (let i = 0; i < values.length; i++) {
    const parsed: any = JSON.parse('{"text":"' + values[i] + '"}');
    console.log('plain', i, JSON.stringify(parsed));
    console.log('pretty', i, JSON.stringify(parsed, null, 2));
    console.log('callback', i, JSON.stringify(parsed, identity));
}
