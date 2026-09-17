# Homebrew formula for daedalus — installs the prebuilt release binary.
#
# To publish, fill the four sha256 values below from a GitHub release, then
# add this file to a Homebrew tap:
#
#   gh release download v0.7.0 -p "daedalus_0.7.0_*_amd64.tar.gz" -p "daedalus_0.7.0_*_arm64.tar.gz"
#   for f in daedalus_0.7.0_*_amd64.tar.gz daedalus_0.7.0_*_arm64.tar.gz; do
#     case "$f" in
#       *linux_amd64*)   echo "linux amd64: $(sha256sum "$f" | cut -d' ' -f1)" ;;
#       *linux_arm64*)   echo "linux arm64: $(sha256sum "$f" | cut -d' ' -f1)" ;;
#       *darwin_amd64*)  echo "darwin amd64: $(shasum -a 256 "$f" | cut -d' ' -f1)" ;;
#       *darwin_arm64*)  echo "darwin arm64: $(shasum -a 256 "$f" | cut -d' ' -f1)" ;;
#     esac
#   done
#
# The stub ships with the CLI in the same archive, so `daedalus build` finds
# it next to the binary.

class Daedalus < Formula
  desc "Package any app into a single self-extracting binary"
  homepage "https://github.com/Encapsul/daedalus"
  url "https://github.com/Encapsul/daedalus/releases"
  license "MIT"
  version "0.7.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Encapsul/daedalus/releases/download/v0.7.0/daedalus_0.7.0_darwin_arm64.tar.gz"
      sha256 "REPLACE_WITH_DARWIN_ARM64_SHA256"
    else
      url "https://github.com/Encapsul/daedalus/releases/download/v0.7.0/daedalus_0.7.0_darwin_amd64.tar.gz"
      sha256 "REPLACE_WITH_DARWIN_AMD64_SHA256"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/Encapsul/daedalus/releases/download/v0.7.0/daedalus_0.7.0_linux_arm64.tar.gz"
      sha256 "REPLACE_WITH_LINUX_ARM64_SHA256"
    else
      url "https://github.com/Encapsul/daedalus/releases/download/v0.7.0/daedalus_0.7.0_linux_amd64.tar.gz"
      sha256 "REPLACE_WITH_LINUX_AMD64_SHA256"
    end
  end

  def install
    dir = Dir["daedalus_*"][0] # release tarballs carry a versioned folder
    bin.install "#{dir}/daedalus", "daedalus"
    bin.install "#{dir}/daedalus-stub", "daedalus-stub"
    bin.install "#{dir}/daedalus-crypto", "daedalus-crypto" if File.exist?("#{dir}/daedalus-crypto")
  end

  test do
    system "#{bin}/daedalus", "--version"
  end
end