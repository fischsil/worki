#!/bin/sh
# Prints the Homebrew cask for a release: scripts/homebrew_cask.sh <version> <sha256 of the DMG>.
# The release workflow writes it to Casks/worki.rb in silvanfischer/homebrew-tap.
set -eu
version=$1
sha256=$2

cat <<EOF
cask "worki" do
  version "$version"
  sha256 "$sha256"

  url "https://github.com/silvanfischer/worki/releases/download/v#{version}/worki_#{version}_universal.dmg"
  name "worki"
  desc "Run coding agents in parallel, each in its own git worktree and sandbox"
  homepage "https://silvanfischer.github.io/worki/"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates true
  depends_on macos: :ventura

  app "worki.app"

  zap trash: [
    "~/Library/Application Support/worki",
    "~/Library/Caches/io.fibe.worki",
    "~/Library/Preferences/io.fibe.worki.plist",
    "~/Library/Saved Application State/io.fibe.worki.savedState",
    "~/Library/WebKit/io.fibe.worki",
  ]
end
EOF
