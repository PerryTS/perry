# Perry APT Repository Setup

## For the `perry-apt` repo (GitHub Pages)

Create a new repo `PerryTS/perry-apt` with GitHub Pages enabled (from `main` branch).

Initialize it with:

```bash
mkdir perry-apt && cd perry-apt
git init
mkdir -p pool/main/p/perry
mkdir -p dists/stable/main/binary-amd64
mkdir -p dists/stable/main/binary-arm64
touch .nojekyll
echo "APT repository for Perry" > README.md
git add -A && git commit -m "init"
git push -u origin main
```

Enable GitHub Pages: Settings → Pages → Source: Deploy from branch → main → / (root)

## User install instructions

```bash
# Add GPG key
curl -fsSL https://perryts.github.io/perry-apt/perry.gpg.pub | sudo gpg --dearmor -o /usr/share/keyrings/perry.gpg

# Add repository
echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/perry.gpg] https://perryts.github.io/perry-apt stable main" | sudo tee /etc/apt/sources.list.d/perry.list

# Install
sudo apt update
sudo apt install perry
```

The `perry` package depends on matching `perry-runtime` and `perry-stdlib`
packages. Together they install only the host compiler, core archives and
runtime sidecar under `/usr/bin` and `/usr/lib/perry`. UI, WebAssembly host and
native-extension libraries use the same signed first-use fetch as npm installs,
under `~/.perry/{ui,wasm-host,ext}/<version>/<triple>/`. Set
`PERRY_NO_AUTO_FETCH=1` to require a prior `perry target add --ui`,
`--wasm-host` or `--ext` install. No UI package is an apt dependency.

## Required secrets in PerryTS/perry repo

| Secret | Description |
|--------|-------------|
| `HOMEBREW_TAP_TOKEN` | GitHub PAT with write access to `PerryTS/homebrew-perry` |
| `APT_REPO_TOKEN` | GitHub PAT with write access to `PerryTS/perry-apt` |
| `APT_GPG_PRIVATE_KEY` | ASCII-armored GPG private key for signing |
| `APT_GPG_KEY_ID` | GPG key ID (e.g., `ABCDEF1234567890`) |

## Generating the GPG key

```bash
gpg --batch --gen-key <<EOF
%no-protection
Key-Type: RSA
Key-Length: 4096
Name-Real: Perry Bot
Name-Email: bot@perryts.com
Expire-Date: 0
EOF

# Get key ID
gpg --list-keys bot@perryts.com

# Export private key (add as APT_GPG_PRIVATE_KEY secret)
gpg --armor --export-secret-keys bot@perryts.com

# Export public key (included in apt repo automatically)
gpg --armor --export bot@perryts.com
```
