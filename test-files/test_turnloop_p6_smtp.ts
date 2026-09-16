// turnloop P6 — SMTP on turnloop handles.
//
// ⚠ THIS FIXTURE DOES NOT RUN TODAY, and that is what it documents.
// `transporter.sendMail(...)` throws `TypeError: (number).sendMail is not a
// function` before any native code is reached — on this branch AND on the base
// commit — because `nodemailer.createTransport()` returns a bare handle NUMBER
// (`NR_F64` in the native table), so codegen lowers the call on a primitive
// receiver to a hard throw and the runtime's handle dispatch is never consulted.
// P6 added the two missing dispatch rows (perry-stdlib's `method_dispatch.rs`
// arm and perry-ext-nodemailer's extension); the other half — returning a
// handle-band NaN-boxed pointer so the receiver is an object — belongs with
// whoever owns that binding. See docs/turnloop/p6-report.md, "SMTP".
//
// It is kept because it is the exact reproducer, and because it is what should
// run the moment that half lands.
//
// The server is a `net.Socket` SMTP responder written here rather than a real
// relay, for the same reason P5's server tests drive a raw socket: the point is
// the PROTOCOL — EHLO, AUTH, MAIL FROM, RCPT TO, DATA, dot-stuffing, QUIT — and
// a library on the far side would hide all of it.
//
// What is printed is the command sequence the server saw (normalized) plus what
// `sendMail` resolved with. Nothing host-specific: the port is never printed,
// the generated Message-ID and Date are scrubbed, and the MIME boundary is not
// exercised (a single-part body).
import net from 'node:net';
import nodemailer from 'nodemailer';

type Session = { commands: string[]; message: string[] };

function scrub(line: string): string {
  return line
    .replace(/^Message-ID: <.*>$/i, 'Message-ID: <scrubbed>')
    .replace(/^Date: .*$/i, 'Date: <scrubbed>');
}

// A minimal RFC 5321 server: greeting, EHLO with a capability list, AUTH PLAIN,
// MAIL/RCPT/DATA, QUIT. `reject` makes RCPT TO answer 550 so the rejected-
// recipient path is exercised too.
function serve(sessions: Session[], reject: boolean): Promise<number> {
  const server = net.createServer((sock) => {
    const session: Session = { commands: [], message: [] };
    sessions.push(session);
    let inData = false;
    let buf = '';
    sock.write('220 perry-test ESMTP\r\n');
    sock.on('data', (chunk: Buffer) => {
      buf += chunk.toString('utf8');
      let idx: number;
      while ((idx = buf.indexOf('\r\n')) >= 0) {
        const line = buf.slice(0, idx);
        buf = buf.slice(idx + 2);
        if (inData) {
          if (line === '.') {
            inData = false;
            session.commands.push('DATA-END');
            sock.write('250 2.0.0 Ok: queued as ABC123\r\n');
          } else {
            session.message.push(scrub(line));
          }
          continue;
        }
        const upper = line.toUpperCase();
        if (upper.startsWith('EHLO')) {
          session.commands.push('EHLO');
          sock.write('250-perry-test\r\n250-PIPELINING\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n250-SIZE 10485760\r\n250 AUTH PLAIN LOGIN\r\n');
        } else if (upper.startsWith('AUTH PLAIN')) {
          session.commands.push('AUTH-PLAIN');
          sock.write('235 2.7.0 Authentication successful\r\n');
        } else if (upper.startsWith('MAIL FROM')) {
          session.commands.push(`MAIL:${line.slice('MAIL FROM:'.length).split(' ')[0]}`);
          sock.write('250 2.1.0 Ok\r\n');
        } else if (upper.startsWith('RCPT TO')) {
          session.commands.push(`RCPT:${line.slice('RCPT TO:'.length).split(' ')[0]}`);
          sock.write(reject ? '550 5.1.1 No such user\r\n' : '250 2.1.5 Ok\r\n');
        } else if (upper === 'DATA') {
          session.commands.push('DATA');
          inData = true;
          sock.write('354 End data with <CR><LF>.<CR><LF>\r\n');
        } else if (upper === 'RSET') {
          session.commands.push('RSET');
          sock.write('250 2.0.0 Ok\r\n');
        } else if (upper === 'QUIT') {
          session.commands.push('QUIT');
          sock.write('221 2.0.0 Bye\r\n');
          sock.end();
        } else {
          session.commands.push(`?${upper.split(' ')[0]}`);
          sock.write('502 5.5.2 Not implemented\r\n');
        }
      }
    });
    sock.on('error', () => {});
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      resolve(typeof address === 'object' && address !== null ? address.port : 0);
    });
  });
}

const sessions: Session[] = [];
const port = await serve(sessions, false);

// The transporter is created at MODULE scope on purpose. Inside an `async
// function` body the binding is boxed into an `Any` cell by the
// async-to-generator transform, the static `nodemailer` type is lost, and
// `transporter.verify()` lands in the untyped handle dispatch as
// `(number).verify is not a function`. That is a pre-existing Perry limitation
// (it reproduces on the base commit), not something this phase changed — but a
// fixture that tripped it would be testing the transform rather than SMTP.
const transporter = nodemailer.createTransport({
  host: '127.0.0.1',
  port,
  secure: false,
  auth: { user: 'probe', pass: 'secret' },
});

const rejectingPort = await serve(sessions, true);
const strict = nodemailer.createTransport({ host: '127.0.0.1', port: rejectingPort, secure: false });

async function main() {
  // 1. verify() completes the whole negotiation and reports true.
  const ok = await transporter.verify();
  console.log('verify', ok);
  console.log('verify commands', sessions[0].commands.join(' '));

  // 2. a plain text message.
  const info = await transporter.sendMail({
    from: 'sender@example.com',
    to: 'recipient@example.com',
    subject: 'P6 subject',
    text: 'line one\nline two',
  });
  console.log('send messageId shape', /^<.+@perry>$/.test(info.messageId));
  console.log('send response', info.response);
  const sent = sessions[1];
  console.log('send commands', sent.commands.join(' '));
  console.log('send body', sent.message.filter((l) => l.startsWith('line ')).join('|'));
  console.log('send has-from', sent.message.some((l) => l === 'From: sender@example.com'));
  console.log('send has-to', sent.message.some((l) => l === 'To: recipient@example.com'));
  console.log('send has-subject', sent.message.some((l) => l === 'Subject: P6 subject'));

  // 3. an HTML message picks the html content type.
  await transporter.sendMail({
    from: 'sender@example.com',
    to: 'recipient@example.com',
    subject: 'html',
    html: '<p>hi</p>',
  });
  console.log('html content-type', sessions[2].message.some((l) => /^Content-Type: text\/html/i.test(l)));

  // 4. a line that begins with a dot must be dot-stuffed, or the message ends
  // early. The server strips the stuffing, so what it recorded is the original.
  await transporter.sendMail({
    from: 'sender@example.com',
    to: 'recipient@example.com',
    subject: 'dots',
    text: 'before\n.\n.hidden\nafter',
  });
  const dotted = sessions[3].message;
  console.log('dot-stuffing', dotted.includes('.'), dotted.includes('.hidden'), dotted.includes('after'));

  // 5. a rejected recipient surfaces as a rejection, not a silent success.
  try {
    await strict.sendMail({
      from: 'sender@example.com',
      to: 'nobody@example.com',
      subject: 'x',
      text: 'y',
    });
    console.log('rejected', 'NOT-REJECTED');
  } catch (err) {
    console.log('rejected', (err as Error).message.includes('550'));
  }

  process.exit(0);
}

main();
