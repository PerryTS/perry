const events: string[] = [];
const key: any = {toString() { events.push('key'); return '0'; }};
const descriptor: any = {get value() { events.push('descriptor'); throw new Error('stop'); }};
const rows: any = JSON.parse('[{"id":1}]');
try { Object.defineProperty(rows, key, descriptor); } catch (error) {}
console.log(events.join(','));
events.length = 0;
try { Object.defineProperty(rows, key, 1 as any); } catch (error) {}
console.log(events.join(','));
