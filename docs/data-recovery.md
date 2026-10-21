# Data-loss investigation and recovery

> **Last reviewed:** 2026-10-21 · **Last updated:** 2026-10-21 · **Change:** minor (investigation of #1092; repaired-copy protection for damaged recovery snapshots)

## Status of report #1092

[The upstream report](https://github.com/storytold/deckcraft/issues/1092) describes repeated manual
saves of an 18-slide portfolio, a crash while inserting video on slide 19, only two slides after
reopening, and a PPTX conversion reported as corrupt. The operating system, application version,
original format and sample files are still unknown. The incident has not been reproduced and its
root cause has not been identified. This work does not close that report.

Source inspection found a separate gap: AutoRecover used the native reader that discards damage
warnings, then retained the original file's save destination. A checkpoint missing media or a
retained chart part could therefore open partially and Save could replace a healthier original.
Recovery now uses the checked reader and applies the same repaired-copy policy as ordinary Open:
it reports damage, marks the copy repaired, and clears its implicit save destination. Save asks
for a location; Save As can explicitly save the copy. Healthy snapshots keep their original
destination and normal recovery behavior.

## Preserve evidence before triage

1. Keep copies of the original file, any exported PPTX, the video and available logs. Record file
   names, sizes and modification times. Work on duplicates rather than the only originals.
2. Copy the recovery folder before opening, saving, closing or reinstalling the application.
   On Windows it is `%APPDATA%\DeckCraft\Recovery`; on macOS it is
   `~/Library/Application Support/DeckCraft/Recovery`; on Linux/BSD it is
   `$XDG_DATA_HOME/deckcraft/recovery` or `~/.local/share/deckcraft/recovery`.
   Keep the numbered `.deckcraft` snapshots and `.json` metadata together. These are application
   data, not disposable build/cache files.
3. Record the OS, exact version/build, original file format and path, save/export steps, media
   format and whether the app displayed a save error or repaired-copy warning. Distinguish the
   manually saved file from an automatically recovered document.
4. Recover from copies in a separate folder and save recovered work under a new name. Verify the
   slides and media before choosing a replacement for an original.

Private incident files stay private during triage. Repository regression fixtures must be
contributor-generated or have provable permitted provenance under [AGENTS.md](../AGENTS.md).

## Regression coverage and remaining work

`crates/engine/src/cmd/recovery_tests.rs` covers missing media and missing chart sidecars in small
contributor-generated checkpoints. It checks the warning, repaired/unsaved state, a Save As
request on ordinary Save, byte-for-byte preservation of the original and checkpoint during open
and path selection, and explicit saving of the repaired copy.

A separate bounded workflow saves 18 slides repeatedly in both native and PPTX formats, checks
each reopen, adds unsaved slide 19, and exercises missing/invalid video input plus a failed Save As.
The previous file, in-memory saved/unsaved state and checkpoint must remain intact, and a new
session must recover all 19 checkpointed slides. These are handled failure paths, not a simulation
of the unknown video crash. Existing tests cover unreadable entries, live owners, reopening twice
and serialization/size refusals that retain the previous checkpoint.

Full Rust CI covers these tests on Linux. The separate `Windows recovery` workflow runs the
recovery tests serially on Windows, including destination-replacement failures, without requiring
a desktop-package build. Both run on GitHub; a browser startup check continues to cover WASM.

Manual saves use an exclusively created temporary sibling, flush it with `sync_all`, rename it
over the destination and only then mark the document saved. Recovery's document and metadata
remain separate replacements, not a transaction across both files. Sudden power-loss durability,
real incident media/decoder behavior and the reported corrupt PPTX still require investigation
with evidence. The tests do not certify those cases.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-21 | minor | Record the unresolved #1092 report, preserve-evidence procedure and checked recovery policy; document bounded regressions and their limits |
