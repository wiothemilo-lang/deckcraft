DeckCraft Fork - Windows x64 portable

1. Extract the entire ZIP into a permanent folder.
2. Double-click Create-Desktop-Shortcut.cmd once.
3. Open DeckCraft Fork from your Desktop, or run Launch-DeckCraft.cmd.
   You can also drag a presentation file onto Launch-DeckCraft.cmd.

Rust, Cargo and an installer are not needed to run this build. Building and
updating the executable happens separately on GitHub Actions. The launcher
does not build, download, update or keep a background process running.

Launch-DeckCraft.cmd keeps settings, logs and AutoRecover files under:
  %LOCALAPPDATA%\DeckCraft-Fork\DeckCraft
The original DeckCraft app's settings are separate. Launching the EXE directly
does not apply this separation. The app window still uses the DeckCraft name.

To update, close the app and replace the files in the same portable folder
with a newly extracted build. Your settings and recovery files are elsewhere.
Do not delete the old folder while the app is running. To remove the portable
app, close it and remove its folder and Desktop shortcut. Saved presentations
are not managed by this launcher.

BUILD-INFO.json identifies the source repository, exact commit and build run.
This is a contributor fork of DeckCraft, not an official ArtCraft release.
See LICENSE-MIT, LICENSE-APACHE, NOTICE and ATTRIBUTION.md for credits/terms.
