# Homebrew formula for daedalus — installs the prebuilt release binary.
#
# To publish, fill the four sha256 values below from a GitHub release, then
# add this file to a Homebrew tap:
#
#   gh release download v0.7.1 -p "daedalus_0.7.1_*_amd64.tar.gz" -p "daedalus_0.7.1_*_arm64.tar.gz"
#   for f in daedalus_0.7.1_*_amd64.tar.gz daedalus_0.7.1_*_arm64.tar.gz; do
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
  version "0.7.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Encapsul/daedalus/releases/download/v0.7.1/daedalus_0.7.1_darwin_arm64.tar.gz"
      sha256 "eff660c3d3a3fb2c71333040fa3b7537fe4aee53059e4fd1b31b51b9a90a0681"
    else
      url "https://github.com/Encapsul/daedalus/releases/download/v0.7.1/daedalus_0.7.1_darwin_amd64.tar.gz"
      sha256 "367b7173d30609ab26adfbf57a91d99665bfef5e7f7220d4bddec84cc43118d7"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/Encapsul/daedalus/releases/download/v0.7.1/daedalus_0.7.1_linux_arm64.tar.gz"
      sha256 "93124131364d3e663820018da7b0e41a6e8b38322ad7140632b69d66d1e04c90"
    else
      url "https://github.com/Encapsul/daedalus/releases/download/v0.7.1/daedalus_0.7.1_linux_amd64.tar.gz"
      sha256 "2b7ff59a1c01ef307f5386007ad6856636af2092e8a00f41a0b4c408503825f2"
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