# Homebrew cask template: the release workflow fills version and sha256 from the published checksums.
cask "kelta" do
  version "0.1.0"
  sha256 :no_check

  url "https://github.com/InjustFr/kelta/releases/download/v#{version}/Kelta_#{version}_universal.dmg"
  name "Kelta"
  desc "Claude Code and nvim side by side, with tickets and pull request reviews"
  homepage "https://github.com/InjustFr/kelta"

  depends_on macos: ">= :ventura"

  app "Kelta.app"
  binary "#{appdir}/Kelta.app/Contents/MacOS/kelta-ctl"

  zap trash: [
    "~/.config/kelta",
    "~/Library/Application Support/dev.kelta.Kelta",
    "~/Library/Logs/Kelta",
  ]
end
