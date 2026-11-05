class Perry < Formula
  desc "Native TypeScript compiler — compiles TypeScript to native executables"
  homepage "https://github.com/PerryTS/perry"
  version "0.2.173"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/PerryTS/perry/releases/download/v0.2.173/perry-macos-aarch64.tar.gz"
      sha256 "PLACEHOLDER"
    else
      url "https://github.com/PerryTS/perry/releases/download/v0.2.173/perry-macos-x86_64.tar.gz"
      sha256 "PLACEHOLDER"
    end
  end

  on_linux do
    url "https://github.com/PerryTS/perry/archive/refs/tags/v0.2.173.tar.gz"
    sha256 "PLACEHOLDER"
    depends_on "rust" => :build
    depends_on "node" => :build
  end

  def install
    if OS.mac?
      bin.install "perry"
      lib.install Dir["libperry_*.a"] + Dir["libperry_*.a.gc-effects"]
    else
      system "cargo", "build", "--release"
      system "cargo", "build", "--release", "-p", "perry-runtime-static", "-p", "perry-stdlib-static"
      system "cargo", "run", "--release", "-p", "perry-gc-effects", "--", "sidecars", "target/release"
      bin.install "target/release/perry"
      system "node", "scripts/publish/archive-kind.mts", "--stage-host", "target/release", "host-staging"
      lib.install Dir["host-staging/libperry_*.a"] + Dir["host-staging/libperry_*.a.gc-effects"]
    end
  end

  def caveats
    <<~EOS
      Perry requires a C linker to link compiled executables.

      macOS:  Xcode Command Line Tools (xcode-select --install)
      Linux:  GCC or Clang (sudo apt install build-essential)

      Quick start:
        echo 'console.log("hello")' > hello.ts
        perry hello.ts -o hello && ./hello
    EOS
  end

  test do
    assert_match "perry", shell_output("#{bin}/perry --version")
    (testpath/"test.ts").write('console.log("works");')
    system bin/"perry", testpath/"test.ts", "-o", testpath/"test"
    assert_equal "works\n", shell_output(testpath/"test")
  end
end
