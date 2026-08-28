# Cross-Platform Development Guide

> **Historical snapshot (v0.2.77).** This file is retained for early Docker
> and CI context; it is not the current installation or release runbook. Use
> [`src/getting-started/installation.md`](src/getting-started/installation.md),
> [`src/contributing/building.md`](src/contributing/building.md), and
> [`src/contributing/releasing.md`](src/contributing/releasing.md) for current
> toolchains and platform coverage.

This guide covers how to develop Perry on macOS while compiling and deploying to Linux/Ubuntu targets.

## Quick Start

### Option 1: GitHub Actions (Recommended for Production)

Template workflow files are provided in `templates/github-actions/`:
- `ci.yml` - Runs tests on Ubuntu and macOS, uploads build artifacts
- `release.yml` - Builds release binaries when you push a version tag

To use them, copy to `.github/workflows/` in your repository:
```bash
mkdir -p .github/workflows
cp templates/github-actions/*.yml .github/workflows/
```

Then push your code and GitHub Actions will automatically:
- Run tests on both Ubuntu and macOS
- Build release binaries for Linux (x86_64) and macOS (x86_64, aarch64)
- Upload artifacts that can be downloaded

To create a release:
```bash
git tag v0.2.77
git push origin v0.2.77
```

### Option 2: Docker (Local Development)

Build and test for Linux locally:

```bash
# Build the Perry compiler Docker image
docker compose build perry

# Compile a TypeScript file to a Linux binary
docker compose run --rm perry myfile.ts -o myfile

# The compiled binary is now in your current directory
./myfile  # (run on Linux or in Docker)
```

### Option 3: Cross-Compilation from macOS

Install cross-compilation toolchain:

```bash
# Install cross-rs for easier cross-compilation
cargo install cross

# Build for Linux
cross build --release --target x86_64-unknown-linux-gnu
```

## Detailed Workflows

### Development on macOS, Deploy to Ubuntu

1. **Develop and test locally on macOS:**
   ```bash
   cargo build
   cargo test
   cargo run -- test.ts -o test_mac
   ./test_mac  # Test on macOS
   ```

2. **Build Linux binary via Docker:**
   ```bash
   # One-liner to compile for Linux
   docker compose run --rm perry test.ts -o test_linux

   # Or build inside a dev container
   docker compose run --rm perry-dev cargo build --release
   ```

3. **Copy to Ubuntu server:**
   ```bash
   scp test_linux user@ubuntu-server:/path/to/app
   scp target/release/libperry_runtime.a user@ubuntu-server:/path/to/app
   ```

### Testing with Services (MySQL, Redis, PostgreSQL)

The docker-compose includes test databases:

```bash
# Start services
docker compose up -d mysql redis postgres

# Wait for services to be healthy
docker compose ps

# Run tests with database connectivity
docker compose run --rm perry-dev cargo test

# Or run specific integration tests
docker compose run --rm perry-dev bash -c "
  cargo run --release -- test_mysql.ts -o test_mysql
  ./test_mysql
"
```

### Interactive Development in Docker

```bash
# Enter a dev shell with full Rust toolchain
docker compose run --rm perry-dev bash

# Inside container:
cargo build --release
cargo test
cargo run -- /app/myfile.ts -o /app/myfile
```

## CI/CD Pipeline

### GitHub Actions Workflows

Template workflows are in `templates/github-actions/`. Copy them to `.github/workflows/` to activate.

**`ci.yml`** - Runs on every push/PR:
- Checks formatting (`cargo fmt`)
- Runs linter (`cargo clippy`)
- Builds and tests on Ubuntu and macOS
- Uploads build artifacts

**`release.yml`** - Runs on version tags:
- Builds release binaries for all platforms
- Creates GitHub Release with downloadable archives
- Generates release notes automatically

### Creating a Release

```bash
# 1. Update version in Cargo.toml and CLAUDE.md
# 2. Commit changes
git add -A && git commit -m "Release v0.2.77"

# 3. Create and push tag
git tag v0.2.77
git push origin main --tags
```

## Alternative Approaches

### 1. GitHub Codespaces

Create a `.devcontainer/devcontainer.json`:
```json
{
  "name": "Perry Dev",
  "image": "mcr.microsoft.com/devcontainers/rust:1",
  "features": {
    "ghcr.io/devcontainers/features/docker-in-docker:2": {}
  },
  "postCreateCommand": "cargo build"
}
```

### 2. Remote Development (VS Code)

Use VS Code Remote - SSH to develop directly on an Ubuntu machine:
1. Install "Remote - SSH" extension
2. Connect to your Ubuntu server
3. Clone and work on the code directly

### 3. Multipass (Ubuntu VMs on macOS)

```bash
# Install multipass
brew install multipass

# Create Ubuntu VM
multipass launch --name perry-dev --cpus 4 --memory 8G --disk 50G

# Mount your code
multipass mount /path/to/perry perry-dev:/home/ubuntu/perry

# SSH into VM
multipass shell perry-dev

# Inside VM: install Rust and build
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cd ~/perry && cargo build --release
```

### 4. Lima (Lightweight VMs on macOS)

```bash
# Install lima
brew install lima

# Start Ubuntu VM
limactl start --name=perry template://ubuntu

# Run commands in VM
lima cargo build --release
```

### 5. Nix with Cross-Compilation

Create a `flake.nix` for reproducible builds across platforms:
```nix
{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }: {
    packages.x86_64-linux.default =
      nixpkgs.legacyPackages.x86_64-linux.callPackage ./default.nix {};
    packages.x86_64-darwin.default =
      nixpkgs.legacyPackages.x86_64-darwin.callPackage ./default.nix {};
  };
}
```

## Docker Image Variants

The Dockerfile provides multiple stages:

| Stage | Use Case | Size |
|-------|----------|------|
| `builder` | Building Perry from source | Large |
| `compiler` | Compiling TypeScript files | ~500MB |
| `runtime` | Running compiled binaries only | ~100MB |

### Using Runtime Image for Deployment

```dockerfile
# In your app's Dockerfile
FROM perry:compiler AS builder
COPY app.ts /app/app.ts
RUN perry /app/app.ts -o /app/app

FROM perry:runtime
COPY --from=builder /app/app /app/app
CMD ["/app/app"]
```

## Troubleshooting

### Linker Errors on Linux

Ensure `libperry_runtime.a` is in the library path:
```bash
export PERRY_RUNTIME_LIB=/path/to/libperry_runtime.a
```

### Binary Won't Run on Ubuntu

Check glibc compatibility:
```bash
ldd ./myprogram
```

If missing `libc.so.6`, the binary was compiled against a newer glibc. Use an older base image (e.g., `debian:bullseye` instead of `bookworm`).

### SSL/TLS Errors

Install CA certificates:
```bash
apt-get install ca-certificates
```
