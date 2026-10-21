# Run the Windows fork without Rust

> **Last reviewed:** 2026-10-21 · **Last updated:** 2026-10-21 · **Change:** minor (manual updater and CI-gated main releases)

The `Windows portable fork` workflow builds a Windows x64 release executable on GitHub.
The laptop only downloads the finished application; opening it never invokes Cargo.
This contributor build is separate from an official ArtCraft release.

## Download and launch

1. In your fork's **Releases**, download a published `fork-windows-<commit>` build, or
   in **Actions**, select **Windows portable fork** and a successful run.
   Use **Run workflow** to build a selected branch after the workflow is on the default branch.
   Packaging changes also build on pull requests.
2. Download the `DeckCraft-Fork-Windows-x64` artifact. GitHub requires a signed-in account.
3. Extract the artifact, then extract its `DeckCraft-Fork-Windows-x64.zip` into a permanent folder.
4. Run `Create-Desktop-Shortcut.cmd` once, then open **DeckCraft Fork** on the Desktop.
   Alternatively, run `Launch-DeckCraft.cmd`; a presentation can be dropped onto that file.

The launcher uses small Windows command/PowerShell scripts. It starts the release EXE and exits;
there is no Rust runtime, launcher framework, background updater or local build cache.
The build uses a static C runtime, matching the existing Windows packaging script.
The application itself still has the same memory requirements and DeckCraft window name.

The launcher sets `APPDATA` only for its child process, so preferences, logs and AutoRecover
live under `%LOCALAPPDATA%\DeckCraft-Fork\DeckCraft`, separate from the original app.
Running `DeckCraft-Fork.exe` directly uses the original app's normal settings directory instead.
The shortcut points to the extracted folder: moving it requires recreating the shortcut.

## Update from the fork

Open **DeckCraft Fork - Update** on the Desktop, or `Update-DeckCraft.cmd` in the portable folder.
It checks public releases from `wiothemilo-lang/deckcraft` without GitHub CLI, a login or Rust.
Save and close the app when prompted; the updater never forces an app to close.
Normal launches make no network request and there is no background updater.

Every push to the fork's `main` triggers a Windows build. Publication requires a successful
full `ci.yml` run on that exact main commit. Only the separate publishing job gets release-write
permission; pull-request builds cannot publish. Assets are uploaded to a draft first, then
published together as a prerelease tagged `fork-windows-<40-character commit>`.
An update becomes available after build, CI and publication finish, not when a commit arrives.
Manual runs on main can retry publication; published releases are not overwritten on rerun.

The updater validates the repository, release asset addresses, manifest, ZIP size/SHA-256 and
embedded build commit. It accepts only the expected flat package files, rejects duplicate/link/
traversal entries, caps downloads at 128 MiB and expanded packages at 256 MiB, and requires 1 GiB
free. A new version is staged under `.versions/<commit>` and startup-tested before an atomic
`.current-version` switch. Download, extraction or startup failure preserves the active version.
Ownership is persisted before candidate movement; a retry can reclaim an interrupted, non-active
candidate of the same release, while preserving active versions and directories with user files.
A lock serializes launcher/update operations; launcher processes exit after starting the app.

The original base package is retained, and unused downloaded versions are cleaned only when
they carry our ownership marker and contain only package files. User-added files are preserved.
Temporary downloads are cleaned on exit; owned leftovers from interrupted downloads are cleaned
on the next update. Settings, AutoRecover and saved presentations are outside this cleanup.
The original base EXE and one clean active downloaded version normally use around 180 MiB
together; download/staging needs extra space temporarily. Directly running an old EXE bypasses
version selection: use the regular Desktop shortcut or `Launch-DeckCraft.cmd`.

## Build provenance

Settings and recovery files stay in the separate user directory. The workflow creates `BUILD-INFO.json` with the
source repository, exact commit (the tested merge commit for a PR) and build run URL.
It also includes the project licences, attribution and notices, and a SHA-256 file for the ZIP.
Artifacts expire after 14 days; a later manual workflow run can build a new copy.

The workflow smoke-tests `--help` and `--version` with Rust removed from `PATH`.
That checks startup and argument handling; it does not establish GUI or rendering correctness
on a particular laptop GPU. Full quality gates remain in the separate `ci` workflow.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-21 | minor | Interrupted activation can retry the same release; ownership precedes movement |
| 2026-10-21 | minor | Desktop update action, bounded staged installs, automatic main builds and CI-gated public releases |
| 2026-10-21 | minor | Windows x64 portable fork bundle, Desktop shortcut and separate preferences/recovery |
