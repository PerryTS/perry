// Large escaping through native buffers, including allocating replacers.
function run(): void {
    let large = '';
    for (let i = 0; i < 512; i++) large += 'line\n"quote"\\tab\t東京한🙂\u0001';
    const longKey = large + '-key';
    const input: any = {};
    input[longKey] = large;
    input.tail = 'end';
    function allocating(key: string, value: any): any {
        if (key.length > 256 && typeof value === 'string') {
            const churn: any[] = [];
            for (let j = 0; j < 64; j++) churn.push({id: j, text: 'churn-' + j});
            const parsed: any = JSON.parse('{"id":' + churn[63].id + '}');
            if (parsed.id !== 63) throw new Error('callback churn changed');
            return value + '\nreturned';
        }
        return value;
    }
    console.log('pretty', JSON.stringify(input, null, 2));
    console.log('keys', JSON.stringify(input, [longKey, 'tail'], 2));
    const retained: string[] = [];
    for (let i = 0; i < 8; i++) {
        input[longKey] = large + '\n' + i;
        retained.push(JSON.stringify(input, allocating, i % 2 === 0 ? 0 : 2));
    }
    for (let i = 0; i < retained.length; i++) {
        const parsed: any = JSON.parse(retained[i]);
        if (parsed[longKey] !== large + '\n' + i + '\nreturned' || parsed.tail !== 'end') {
            throw new Error('retained native-buffer output changed');
        }
        console.log('retained-full', i, retained[i]);
    }
    for (let n = 255; n <= 257; n++) {
        let text = '';
        for (let j = 0; j < n; j++) text += j % 3 === 0 ? '\n' : 'a';
        console.log('boundary', n, JSON.stringify({text: text}, null, 2));
        console.log('boundary-callback', n, JSON.stringify({text: text}, allocating, 2));
    }
    const lone = large + '\ud800';
    console.log('lone-pretty', JSON.stringify({text: lone}, null, 2));
    console.log('lone-callback', JSON.stringify({text: lone}, allocating, 2));
}
run();
