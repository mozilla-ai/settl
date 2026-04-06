# Homebrew formula for settl
# This is a template -- the CI workflow (update-homebrew.yml) auto-generates
# the real formula in mozilla-ai/homebrew-tap on each release.
#
# To bootstrap the tap manually:
#   1. Clone git@github.com:mozilla-ai/homebrew-tap.git
#   2. Copy this file to Formula/settl.rb
#   3. Replace VERSION and SHA256 placeholders with real values from a release

class Settl < Formula
  desc "Terminal hex-based settlement game with LLM players"
  homepage "https://mozilla-ai.github.io/settl/"
  version "VERSION"
  license "Apache-2.0"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/mozilla-ai/settl/releases/download/vVERSION/settl-darwin-amd64.tar.gz"
      sha256 "SHA256_DARWIN_AMD64"
    end
    if Hardware::CPU.arm?
      url "https://github.com/mozilla-ai/settl/releases/download/vVERSION/settl-darwin-arm64.tar.gz"
      sha256 "SHA256_DARWIN_ARM64"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/mozilla-ai/settl/releases/download/vVERSION/settl-linux-amd64.tar.gz"
      sha256 "SHA256_LINUX_AMD64"
    end
    if Hardware::CPU.arm?
      url "https://github.com/mozilla-ai/settl/releases/download/vVERSION/settl-linux-arm64.tar.gz"
      sha256 "SHA256_LINUX_ARM64"
    end
  end

  def install
    if OS.mac? && Hardware::CPU.intel?
      bin.install "settl-darwin-amd64" => "settl"
    elsif OS.mac? && Hardware::CPU.arm?
      bin.install "settl-darwin-arm64" => "settl"
    elsif OS.linux? && Hardware::CPU.intel?
      bin.install "settl-linux-amd64" => "settl"
    elsif OS.linux? && Hardware::CPU.arm?
      bin.install "settl-linux-arm64" => "settl"
    end
  end

  test do
    assert_match "settl", shell_output("#{bin}/settl --help")
  end
end
