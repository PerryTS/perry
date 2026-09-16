#!/bin/bash
# TLS for the P7 lane's four private database servers.
#
# P7 brought the servers up without TLS (`/root/claude-turnloop-p7/dbservers.sh`).
# This adds a private CA, one leaf per server, and turns TLS on in a way that
# keeps every existing PLAINTEXT fixture working:
#
#   postgres 55432  ssl=on          same port (SSLRequest negotiates per connection)
#   mysql    53306  --ssl-*         same port (SSLRequest capability flag mid-handshake)
#   redis    56379 + tls 56380      SEPARATE port: redis has no in-band upgrade
#   mongodb  57017  --tlsMode preferTLS   same port (accepts both)
#
# The leaf certificates are RSA/SHA-256 on purpose: RFC 5929 tls-server-end-point
# is only defined for signature algorithms whose hash is known, and an Ed25519
# leaf makes `turnloop_tls::tls_server_end_point` return None — which would make
# a SCRAM-SHA-256-PLUS proof silently fall back to plain SCRAM.
set -eu
ROOT=/srv/claude-turnloop-p7-servers
CERTS=$ROOT/tls
PGBIN=/usr/lib/postgresql/16/bin
PGDATA=$ROOT/pg
MYDATA=$ROOT/mysql
REDISDIR=$ROOT/redis
MONGODIR=$ROOT/mongo
LOGS=$ROOT/logs
PGPORT=55432; MYPORT=53306; REDISPORT=56379; REDISTLSPORT=56380; MONGOPORT=57017

certs() {
  mkdir -p "$CERTS"
  if [ ! -f "$CERTS/ca.crt" ]; then
    openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
      -keyout "$CERTS/ca.key" -out "$CERTS/ca.crt" \
      -subj "/CN=perry-turnloop-test-ca" \
      -addext "basicConstraints=critical,CA:TRUE" \
      -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
  fi
  for name in pg mysql redis mongo; do
    [ -f "$CERTS/$name.crt" ] && continue
    openssl req -newkey rsa:2048 -sha256 -nodes \
      -keyout "$CERTS/$name.key" -out "$CERTS/$name.csr" \
      -subj "/CN=localhost" 2>/dev/null
    openssl x509 -req -in "$CERTS/$name.csr" -CA "$CERTS/ca.crt" -CAkey "$CERTS/ca.key" \
      -CAcreateserial -out "$CERTS/$name.crt" -days 3650 -sha256 \
      -extfile <(printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n') 2>/dev/null
    rm -f "$CERTS/$name.csr"
  done
  # mongod wants one combined PEM.
  cat "$CERTS/mongo.key" "$CERTS/mongo.crt" > "$CERTS/mongo.pem"
  chmod 644 "$CERTS"/*.crt "$CERTS"/*.pem
  chmod 640 "$CERTS"/*.key
  # Each server reads its own key as its own user.
  install -m 600 -o postgres -g postgres "$CERTS/pg.key" "$PGDATA/server.key"
  install -m 644 -o postgres -g postgres "$CERTS/pg.crt" "$PGDATA/server.crt"
  install -m 600 -o mysql -g mysql "$CERTS/mysql.key" "$MYDATA/perry-server-key.pem"
  install -m 644 -o mysql -g mysql "$CERTS/mysql.crt" "$MYDATA/perry-server-cert.pem"
  install -m 644 -o mysql -g mysql "$CERTS/ca.crt" "$MYDATA/perry-ca.pem"
  chmod 644 "$CERTS/redis.key" "$CERTS/mongo.pem"
  echo "certs ok: $CERTS"
  openssl x509 -in "$CERTS/pg.crt" -noout -subject -issuer -dates \
    -ext subjectAltName 2>/dev/null | sed 's/^/  /'
}

configure() {
  # --- postgres: ssl on, same port ---
  grep -q '^ssl = on' "$PGDATA/postgresql.conf" || cat >> "$PGDATA/postgresql.conf" <<'CONF'

# perry turnloop TLS lane
ssl = on
ssl_cert_file = 'server.crt'
ssl_key_file = 'server.key'
password_encryption = scram-sha-256
CONF
  echo "postgres configured"
}

start() {
  mkdir -p "$LOGS"
  su postgres -c "$PGBIN/pg_ctl -D $PGDATA -o '-p $PGPORT -c listen_addresses=127.0.0.1 -c unix_socket_directories=$PGDATA' -l $LOGS/pg.log restart -m fast" >/dev/null 2>&1 || \
  su postgres -c "$PGBIN/pg_ctl -D $PGDATA -o '-p $PGPORT -c listen_addresses=127.0.0.1 -c unix_socket_directories=$PGDATA' -l $LOGS/pg.log start" >/dev/null 2>&1

  mysqladmin --protocol=TCP -h 127.0.0.1 -P $MYPORT -u root -pperrypw shutdown 2>/dev/null || true
  sleep 2
  setsid mysqld --user=mysql --datadir="$MYDATA" --port=$MYPORT --bind-address=127.0.0.1 \
    --socket="$ROOT/mysqlrun/mysql.sock" --mysqlx=OFF --pid-file="$ROOT/mysqlrun/mysqld.pid" \
    --log-error="$LOGS/mysql.log" --skip-name-resolve \
    --ssl-ca="$MYDATA/perry-ca.pem" --ssl-cert="$MYDATA/perry-server-cert.pem" \
    --ssl-key="$MYDATA/perry-server-key.pem" </dev/null >/dev/null 2>&1 &

  redis-cli -p $REDISPORT shutdown nosave 2>/dev/null || true
  sleep 1
  setsid redis-server --port $REDISPORT --bind 127.0.0.1 --dir "$REDISDIR" \
    --tls-port $REDISTLSPORT --tls-cert-file "$CERTS/redis.crt" \
    --tls-key-file "$CERTS/redis.key" --tls-ca-cert-file "$CERTS/ca.crt" \
    --tls-auth-clients no \
    --daemonize no --logfile "$LOGS/redis.log" --save '' </dev/null >/dev/null 2>&1 &

  mongosh --port $MONGOPORT --quiet --eval 'db.getSiblingDB("admin").shutdownServer()' 2>/dev/null || true
  sleep 2
  # BOTH flags, and neither alone works. Without --tlsCAFile mongod 8 refuses
  # to start ("The use of TLS without specifying a chain of trust is no longer
  # supported", SERVER-72839); with a CA and nothing else it REQUIRES a client
  # certificate and refuses every client, mongosh included, with "No SSL
  # certificate provided by peer". This fixture authenticates the server only.
  setsid mongod --port $MONGOPORT --bind_ip 127.0.0.1 --dbpath "$MONGODIR" \
    --tlsMode preferTLS --tlsCertificateKeyFile "$CERTS/mongo.pem" \
    --tlsCAFile "$CERTS/ca.crt" --tlsAllowConnectionsWithoutCertificates \
    --logpath "$LOGS/mongo.log" </dev/null >/dev/null 2>&1 &
  sleep 14
  verify
}

verify() {
  echo "--- plaintext ports ---"
  for p in $PGPORT $MYPORT $REDISPORT $MONGOPORT $REDISTLSPORT; do
    if (echo >/dev/tcp/127.0.0.1/$p) 2>/dev/null; then echo "port $p UP"; else echo "port $p DOWN"; fi
  done
  echo "--- postgres TLS ---"
  PGPASSWORD=perrypw $PGBIN/psql "host=127.0.0.1 port=$PGPORT user=perry dbname=postgres sslmode=require sslrootcert=$CERTS/ca.crt" \
    -tAc "SELECT 'pgtls ' || ssl || ' ' || version || ' ' || cipher FROM pg_stat_ssl WHERE pid = pg_backend_pid()" 2>&1 | tail -2
  PGPASSWORD=perrypw $PGBIN/psql "host=127.0.0.1 port=$PGPORT user=perry dbname=postgres sslmode=require sslrootcert=$CERTS/ca.crt" \
    -tAc "SELECT 'pgauth ' || a.usename || ' ' || COALESCE(s.ssl::text,'?') FROM pg_stat_activity a JOIN pg_stat_ssl s USING (pid) WHERE a.pid = pg_backend_pid()" 2>&1 | tail -1
  echo "--- mysql TLS ---"
  mysql --protocol=TCP -h 127.0.0.1 -P $MYPORT -u perry -pperrypw --ssl-mode=REQUIRED \
    -e "SHOW STATUS LIKE 'Ssl_cipher'" 2>&1 | grep -v "^mysql:" | tail -2
  echo "--- redis TLS ---"
  redis-cli --tls -p $REDISTLSPORT --cacert "$CERTS/ca.crt" PING 2>&1 | tail -1
  echo "--- mongo TLS ---"
  mongosh --quiet --host 127.0.0.1 --port $MONGOPORT --tls --tlsCAFile "$CERTS/ca.crt" \
    --eval 'db.runCommand({ping:1}).ok' 2>&1 | tail -1
}

case "${1:-all}" in
  certs) certs ;;
  configure) configure ;;
  start) start ;;
  verify) verify ;;
  all) certs; configure; start ;;
  *) echo "usage: $0 certs|configure|start|verify|all" ;;
esac
