/**
 * @file Pure Homebrew-formula renderer for Perry, modeled on a tiered registry-infra design
 *   adapted to Perry's asset naming +
 *   from-source linux build. Every input arrives as data — no filesystem, no
 *   network, no gh — so the whole surface is unit-testable without a tap.
 *
 *   Perry ships prebuilt macOS binaries (perry-macos-aarch64.tar.gz,
 *   perry-macos-x86_64.tar.gz) but the Homebrew formula builds from source on
 *   Linux (cargo build from the tag archive) — matching the existing
 *   packaging/homebrew/perry.rb shape.
 */

export interface PerryFormulaSpec {
  /** Bare version, no leading v — `0.5.1510`. */
  version: string
  /** sha256 of the macos-aarch64 tarball. */
  macosArm64Sha256: string
  /** sha256 of the macos-x86_64 tarball. */
  macosX64Sha256: string
  /** sha256 of the source archive (archive/refs/tags/vX.Y.Z.tar.gz) for linux. */
  linuxSourceSha256: string
}

export const RELEASE_REPO = 'PerryTS/perry'

/** Download URL for a release asset. */
export function assetUrl(tag: string, assetName: string): string {
  return `https://github.com/${RELEASE_REPO}/releases/download/${tag}/${assetName}`
}

/** Source archive URL for the linux from-source build. */
export function sourceArchiveUrl(version: string): string {
  return `https://github.com/${RELEASE_REPO}/archive/refs/tags/v${version}.tar.gz`
}

/**
 * Render Formula/perry.rb. Byte-identical regenerations when nothing moved —
 * the tap bump stays diff-quiet. Mirrors the existing perry.rb shape.
 */
export function renderPerryFormula(spec: PerryFormulaSpec): string {
  const tag = `v${spec.version}`
  return [
    'class Perry < Formula',
    '  desc "Native TypeScript compiler — compiles TypeScript to native executables"',
    `  homepage "https://github.com/${RELEASE_REPO}"`,
    `  version "${spec.version}"`,
    '  license "MIT"',
    '',
    '  on_macos do',
    '    on_arm do',
    `      url "${assetUrl(tag, 'perry-macos-aarch64.tar.gz')}"`,
    `      sha256 "${spec.macosArm64Sha256}"`,
    '    end',
    '    on_intel do',
    `      url "${assetUrl(tag, 'perry-macos-x86_64.tar.gz')}"`,
    `      sha256 "${spec.macosX64Sha256}"`,
    '    end',
    '  end',
    '',
    '  on_linux do',
    `    url "${sourceArchiveUrl(spec.version)}"`,
    `    sha256 "${spec.linuxSourceSha256}"`,
    '    depends_on "rust" => :build',
    '  end',
    '',
    '  def install',
    '    if OS.mac?',
    '      bin.install "perry"',
    '      lib.install Dir["libperry_*.a"]',
    '    else',
    '      system "cargo", "build", "--release"',
    '      system "cargo", "build", "--release", "-p", "perry-runtime", "-p", "perry-stdlib"',
    '      bin.install "target/release/perry"',
    '      lib.install Dir["target/release/libperry_*.a"]',
    '    end',
    '  end',
    '',
    '  def caveats',
    '    <<~EOS',
    '      Perry requires a C linker to link compiled executables.',
    '',
    '      macOS:  Xcode Command Line Tools (xcode-select --install)',
    '      Linux:  GCC or Clang (sudo apt install build-essential)',
    '',
    '      Quick start:',
    "        echo 'console.log(\"hello\")' > hello.ts",
    '        perry hello.ts -o hello && ./hello',
    '    EOS',
    '  end',
    '',
    '  test do',
    '    assert_match "perry", shell_output("#{bin}/perry --version")',
    '    (testpath/"test.ts").write(\'console.log("works");\')',
    '    system bin/"perry", testpath/"test.ts", "-o", testpath/"test"',
    '    assert_equal "works\\n", shell_output(testpath/"test")',
    '  end',
    'end',
    '',
  ].join('\n')
}
