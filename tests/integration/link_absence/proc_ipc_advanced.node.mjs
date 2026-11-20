import {spawn} from 'node:child_process';
const command=JSON.parse(process.argv[2]);
const child=spawn(command[0],command.slice(1),{stdio:['ignore','pipe','pipe','ipc'],serialization:'advanced'});
child.stdout.pipe(process.stdout);child.stderr.pipe(process.stderr);
const deadline=setTimeout(()=>{child.kill();process.exitCode=1},4000);
child.once('spawn',()=>child.send(new Int32Array([42,-7])));
child.once('error',error=>{clearTimeout(deadline);throw error});
child.once('exit',code=>{clearTimeout(deadline);process.exitCode=code??1});
