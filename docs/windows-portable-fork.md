# Run the Windows fork without Rust

> **Last reviewed:** 2026-10-21 · **Last updated:** 2026-10-21 · **Change:** minor (portable fork launcher and cloud build)

The `Windows portable fork` workflow builds a Windows x64 release executable on GitHub.
The laptop only downloads the finished application; opening it never invokes Cargo.
This contributor build is separate from an official ArtCraft release.

## Download and launch

1. In your fork's **Actions**, select **Windows portable fork** and a successful run.
   Use **Run workflow** to build a selected branch after the workflow is on the default branch.
   Packaging changes also build on pull requests.
2. Download the `DeckCraft-Fork-Windows-x64` artifact. GitHub requires a signed-in account.
3. Extract the artifact, then extract its `DeckCraft-Fork-Windows-x64.zip` into a permanent folder.
4. Run `Create-Desktop-Shortcut.cmd` once, then open **DeckCraft Fork** on the Desktop.
   Alternatively, run `Launch-DeckCraft.cmd`; a presentation can be dropped onto that file.

The launcher is a small Windows command script. It starts the release EXE and exits;
there is no Rust runtime, launcher framework, background updater or local build cache.
The build uses a static C runtime, matching the existing Windows packaging script.
The application itself still has the same memory requirements and DeckCraft window name.

The launcher sets `APPDATA` only for its child process, so preferences, logs and AutoRecover
live under `%LOCALAPPDATA%\DeckCraft-Fork\DeckCraft`, separate from the original app.
Running `DeckCraft-Fork.exe` directly uses the original app's normal settings directory instead.
The shortcut points to the extracted folder: moving it requires recreating the shortcut.

## Updates and provenance

Close the app before replacing the portable files with a newer build. Settings and recovery
files stay in the separate user directory. The workflow creates `BUILD-INFO.json` with the
source repository, exact commit (the tested merge commit for a PR) and build run URL.
It also includes the project licences, attribution and notices, and a SHA-256 file for the ZIP.
Artifacts expire after 14 days; a later manual workflow run can build a new copy.

The workflow smoke-tests `--help` and `--version` with Rust removed from `PATH`.
That checks startup and argument handling; it does not establish GUI or rendering correctness
on a particular laptop GPU. Full quality gates remain in the separate `ci` workflow.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-21 | minor | Windows x64 portable fork bundle, Desktop shortcut and separate preferences/recovery |
