DeckCraft Fork - Windows x64 portable

1. Extract the entire ZIP into a permanent folder.
2. Double-click Create-Desktop-Shortcut.cmd once.
3. Open DeckCraft Fork from your Desktop, or run Launch-DeckCraft.cmd.
   You can also drag a presentation file onto Launch-DeckCraft.cmd.
4. For updates, use DeckCraft Fork - Update on the Desktop, or run
   Update-DeckCraft.cmd. Save and close the app before installing.

Rust, Cargo and an installer are not needed to run this build. Building and
updating the executable happens separately on GitHub Actions. The normal
launcher does not download or build anything. The updater runs only when you
open it; there is no background updater. It needs internet but no GitHub login.

Launch-DeckCraft.cmd keeps settings, logs and AutoRecover files under:
  %LOCALAPPDATA%\DeckCraft-Fork\DeckCraft
The original DeckCraft app's settings are separate. Launching the EXE directly
does not apply this separation. The app window still uses the DeckCraft name.

Updates come only from published Windows builds of wiothemilo-lang/deckcraft
main, after full CI succeeds. A new repo commit is not immediately a new app:
the cloud build and publication must finish first. The updater checks the ZIP
checksum and build identity, tests startup, then switches the active version.
A failed download or startup leaves the current version selected.

Downloaded versions live in .versions. The updater keeps the original base
package and the active version, removes unused managed versions and cleans
temporary downloads. Folders with user-added files are preserved. Keep at
least 1 GB free for an update. Settings/recovery and saved decks stay separate.
Do not delete the old folder while the app is running. To remove the portable
app, close it and remove its folder and Desktop shortcut. Saved presentations
are not managed by this launcher.

BUILD-INFO.json identifies the source repository, exact commit and build run.
This is a contributor fork of DeckCraft, not an official ArtCraft release.
See LICENSE-MIT, LICENSE-APACHE, NOTICE and ATTRIBUTION.md for credits/terms.
