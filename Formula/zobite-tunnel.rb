# Homebrew formula for Zobite Tunnel Client
# Repo: https://github.com/Zobite/homebrew-tap
#
# Install: brew tap Zobite/tap && brew install zobite-tunnel
# Usage:   zobite-tunnel-client --server vps:7000 --local localhost:3000 --id app --token xxx

class ZobiteTunnel < Formula
  desc "Self-hosted tunnel — expose local services to the internet via your own VPS"
  homepage "https://github.com/Zobite/zo-tunnel"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Zobite/zo-tunnel/releases/download/v#{version}/zobite-tunnel-client-v#{version}-darwin-arm64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_ARM64"
    else
      url "https://github.com/Zobite/zo-tunnel/releases/download/v#{version}/zobite-tunnel-client-v#{version}-darwin-amd64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_AMD64"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/Zobite/zo-tunnel/releases/download/v#{version}/zobite-tunnel-client-v#{version}-linux-arm64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_ARM64"
    else
      url "https://github.com/Zobite/zo-tunnel/releases/download/v#{version}/zobite-tunnel-client-v#{version}-linux-amd64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_AMD64"
    end
  end

  def install
    bin.install "zobite-tunnel-client"
  end

  test do
    assert_match "Zobite Tunnel tunnel client", shell_output("#{bin}/zobite-tunnel-client --help")
  end
end
