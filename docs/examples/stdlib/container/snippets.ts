// demonstrates: per-snippet examples for the perry/container + perry/compose
//   docs page (docs/src/stdlib/container.md)
// docs: docs/src/stdlib/container.md
// platforms: macos, linux, windows
// run: false

// Each ANCHOR block below is the code that the container docs page renders
// inline via {{#include ... :NAME}}. The file as a whole is compiled and
// linked by the doc-tests harness — `run: false` because every example
// touches a live OCI runtime (apple/container, docker, podman, …) which
// isn't hermetic in CI. Compile + link is the contract here; the live
// runtime path is exercised by example-code/forgejo-deployment which is
// run by hand against Docker on the maintainer's machine.

// ANCHOR: backend-detect
import { getBackend, detectBackend } from "perry/container";

async function pickBackend(): Promise<void> {
    // Synchronous: returns the canonical name of the active backend
    // (`"docker"`, `"podman"`, `"apple/container"`, `"orbstack"`,
    // `"colima"`, `"lima"`, `"nerdctl"`, …). When called before any
    // async FFI has triggered detection, getBackend() performs a
    // synchronous in-place probe with the same 2 s timeout per
    // candidate that detectBackend() uses, so the result is live.
    console.log(`backend: ${getBackend()}`);

    // Async + verbose: returns a JSON array of every probed backend
    // with availability + version + reason for unavailable ones. Use
    // this when you want to surface a "diagnostics" panel to the user.
    const probed = await detectBackend();
    console.log(probed);
}
// ANCHOR_END: backend-detect

// ANCHOR: run-simple
import { run, remove } from "perry/container";

async function runAlpine(): Promise<void> {
    const handle = await run({
        image: "alpine:3.19",
        cmd: ["echo", "hello from perry"],
        rm: false,
        // Production-friendly defaults: drop every Linux capability and
        // run as a non-root user. Add `cap_add` only for the specific
        // capabilities a workload actually needs.
        user: "nobody",
        cap_drop: ["ALL"],
    });
    console.log(`container handle: ${String(handle)}`);

    // `force: true` removes the container even if still running (the
    // FFI calls `docker rm -f` / `podman rm -f`).
    await remove(handle as unknown as string, true);
}
// ANCHOR_END: run-simple

// ANCHOR: run-secure
import { run as runSecure } from "perry/container";

// Maximum-isolation single-container run for an untrusted workload:
//   - read-only root filesystem
//   - no Linux capabilities at all
//   - non-root user
//   - working directory pinned
//   - default seccomp profile
async function runUntrustedWorkload(): Promise<void> {
    await runSecure({
        image: "alpine:3.19",
        cmd: ["sh", "-c", "echo isolated && exit 0"],
        read_only: true,
        cap_drop: ["ALL"],
        user: "nobody",
        workdir: "/tmp",
        seccomp: "default",
    });
}
// ANCHOR_END: run-secure

// ANCHOR: list-inspect
import {
    list,
    inspect,
    logs,
    exec,
} from "perry/container";

async function inspectAll(): Promise<void> {
    const containers = await list(true); // all=true → include stopped
    console.log(containers);

    const id = "my-container-id";
    const info = await inspect(id);
    console.log(info.status); // "running" | "exited" | …

    // Tail the last 50 stdout/stderr lines.
    const tailed = await logs(id, { tail: 50 });
    console.log(tailed.stdout);

    // Run a command inside the container; returns a ContainerLogs
    // handle whose stdout/stderr you can read.
    const r = await exec(id, ["ls", "-la"]);
    console.log(r.stdout);
}
// ANCHOR_END: list-inspect

// ANCHOR: image-mgmt
import { pullImage, listImages, removeImage } from "perry/container";

async function manageImages(): Promise<void> {
    await pullImage("postgres:16-alpine");
    const images = await listImages();
    console.log(`${images.length} images`);
    await removeImage("postgres:16-alpine", false);
}
// ANCHOR_END: image-mgmt

// ANCHOR: compose-up-simple
import { up } from "perry/compose";

async function bringUpSimpleStack(): Promise<void> {
    const stack = await up({
        version: "3.8",
        services: {
            cache: {
                image: "redis:7-alpine",
                ports: ["6379:6379"],
                networks: ["app-net"],
                healthcheck: {
                    test: ["CMD", "redis-cli", "PING"],
                    interval: "5s",
                    timeout: "3s",
                    retries: 6,
                },
            },
        },
        networks: {
            "app-net": { driver: "bridge" },
        },
    });
    // `stack` is an opaque handle (NaN-boxed integer) — pass it as
    // the first arg to `down` / `ps` / `logs` / `exec`.
    console.log(`stack handle: ${String(stack)}`);
}
// ANCHOR_END: compose-up-simple

// ANCHOR: compose-up-multi
import { up as upMulti } from "perry/compose";

async function bringUpMultiServiceStack(): Promise<void> {
    // depends_on with `condition: 'service_healthy'` blocks the
    // dependent service until the dependency's healthcheck reports
    // healthy. Use the map form (not the bare-array form) to pass
    // the condition.
    await upMulti({
        version: "3.8",
        services: {
            db: {
                image: "postgres:16-alpine",
                container_name: "app-db", // stable DNS target for siblings
                environment: {
                    POSTGRES_USER:     "app",
                    POSTGRES_PASSWORD: "${APP_DB_PASSWORD:-changeme}",
                    POSTGRES_DB:       "app",
                },
                volumes: ["app-pgdata:/var/lib/postgresql/data"],
                networks: ["app-db-net"],
                healthcheck: {
                    test: ["CMD-SHELL", "pg_isready -U app -d app"],
                    interval: "5s",
                    timeout: "3s",
                    retries: 10,
                    start_period: "30s",
                },
            },
            api: {
                image: "myorg/api:1.0",
                depends_on: { db: { condition: "service_healthy" } },
                environment: {
                    DATABASE_URL: "postgres://app:changeme@app-db:5432/app",
                },
                ports: ["8080:8080"],
                networks: ["app-db-net", "app-web-net"],
                restart: "unless-stopped",
            },
        },
        networks: {
            "app-db-net":  { driver: "bridge", internal: true }, // db unreachable from host
            "app-web-net": { driver: "bridge" },
        },
        volumes: {
            "app-pgdata": { driver: "local" },
        },
    });
}
// ANCHOR_END: compose-up-multi

// ANCHOR: compose-down
import { down } from "perry/compose";

async function tearDown(stack: number): Promise<void> {
    // Default: containers + networks removed; named volumes preserved
    // so a subsequent `up()` against the same spec resumes from
    // committed state.
    await down(stack);

    // Pass `volumes: true` to also drop named volumes — DESTROYS DATA.
    // Useful for test teardown or for a "rip and replace" redeploy.
    await down(stack, { volumes: true });
}
// ANCHOR_END: compose-down

// ANCHOR: compose-ops
import {
    ps,
    logs as composeLogs,
    exec as composeExec,
    config,
    start,
    stop,
    restart,
} from "perry/compose";

async function manageStack(stack: number): Promise<void> {
    // Status of every service in the stack (returns a registry
    // handle to a ContainerInfo[]; user-side array materialisation
    // is a follow-up ergonomics task).
    const statusHandle = await ps(stack);
    console.log(statusHandle);

    // Aggregated logs from one or all services.
    await composeLogs(stack, { service: "db", tail: 200 });

    // Exec a command inside a service's container by service KEY
    // (not container name) — the engine resolves the service to its
    // running container internally.
    await composeExec(stack, "db", ["pg_isready"]);

    // Resolved YAML the engine actually used (post-interpolation).
    const yaml = await config(stack);
    console.log(yaml);

    // Stop / start / restart by service key. `services: []` (or
    // omitted) targets every service in the stack.
    await stop(stack, ["api"]);
    await start(stack, ["api"]);
    await restart(stack, []);
}
// ANCHOR_END: compose-ops

// ANCHOR: env-interpolation
import { up as upEnv } from "perry/compose";

// Compose YAML interpolation (`${VAR}` / `${VAR:-default}`) is applied
// to TS-side specs at the FFI boundary too — set `process.env` keys
// before calling up() and they'll resolve in the spec values.
async function envInterpolatedStack(): Promise<void> {
    await upEnv({
        version: "3.8",
        services: {
            web: {
                image: "nginx:${NGINX_VERSION:-alpine}",
                ports: ["${WEB_PORT:-8080}:80"],
                environment: {
                    SERVER_NAME: "${WEB_DOMAIN:-localhost}",
                },
            },
        },
    });
}
// ANCHOR_END: env-interpolation

// ANCHOR: container-name-dns
// IMPORTANT: Perry's compose engine creates each container with a
// `{md5}-{random_hex}` derived name and DOES NOT (yet) register the
// service KEY (`db`, `api`, …) as a network alias. So
// `DATABASE_URL: 'postgres://user:pw@db:5432/app'` would fail name
// resolution at runtime. Two ways to make sibling-DNS work:
//
//   (a) Set `container_name` explicitly on each service so the
//       chosen name is what Docker's embedded DNS resolves. This is
//       the simplest pattern and is what the Forgejo example uses.
//
//   (b) Wait for service-key network-alias support (planned).
//
// Until (b) lands, prefer (a):
import { up as upDns } from "perry/compose";

async function dnsAwareStack(): Promise<void> {
    await upDns({
        version: "3.8",
        services: {
            db: {
                image: "postgres:16-alpine",
                container_name: "myapp-db", // ← stable DNS target
                networks: ["myapp-net"],
                environment: { POSTGRES_PASSWORD: "x" },
            },
            api: {
                image: "myapp/api",
                container_name: "myapp-api",
                networks: ["myapp-net"],
                environment: {
                    // Use the container_name as the hostname:
                    DATABASE_URL: "postgres://postgres:x@myapp-db:5432/postgres",
                },
            },
        },
        networks: { "myapp-net": { driver: "bridge" } },
    });
}
// ANCHOR_END: container-name-dns
