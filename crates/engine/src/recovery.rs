//! Crash recovery (PowerPoint's AutoRecover): unsaved changes of every open presentation are
//! written to a recovery folder (`file.recovery.save`, called on a timer by the app and after a
//! command panics); saving or closing a presentation removes its entry; after a crash
//! `file.recovery.open` reopens what was left there as unsaved presentations that remember where
//! they were saved.
//!
//! An entry is `<uid>.deckcraft` (the presentation) plus `<uid>.json` (`{"path", "title",
//! "saved", "owner"}`: the original file, the title, when it was written (Unix seconds) and the
//! instance that owns it).
//!
//! Several instances share the folder. Document uids are unique across processes, each instance
//! holds a lock on `owner-<instance>.lock` while it runs, and `file.recovery.open` leaves alone
//! the entries of an instance whose lock is still held. Opening an entry doesn't delete it: the
//! recovered presentation keeps the entry's uid, so the snapshot stays on disk until the next
//! recovery save replaces it or a save of the presentation removes it.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use crate::{DocState, EngineError, Result, Session};

/// The platform's per-user recovery folder.
pub fn default_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|h| h.join("Library/Application Support/DeckCraft/Recovery"));
    }
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DeckCraft").join("Recovery"));
    }
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".local/share"))).map(|d| d.join("deckcraft/recovery"))
}

/// Uids and instance ids stay below 2^53 so they survive JSON numbers read as `f64`.
pub(crate) const UID_MASK: u64 = (1 << 53) - 1;

/// This process's random id, with the low 24 bits clear so its document uids (id + counter) don't
/// run into another instance's.
pub fn instance_id() -> u64 {
    static ID: OnceLock<u64> = OnceLock::new();
    *ID.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let h = std::collections::hash_map::RandomState::new().build_hasher();
        #[cfg(not(target_arch = "wasm32"))]
        let h = {
            // Browser runtimes have neither a process id nor std's wall clock. Document
            // counters still make ids distinct within that runtime; native recovery also
            // mixes in its process and clock so instances sharing a folder stay separate.
            let mut h = h;
            h.write_u32(std::process::id());
            if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                h.write_u128(d.as_nanos());
            }
            h
        };
        // Never 0: old entries are numbered from 1.
        ((h.finish() & UID_MASK) & !0xFF_FFFF).max(1 << 24)
    })
}

fn lock_path(dir: &Path, owner: u64) -> PathBuf {
    dir.join(format!("owner-{owner}.lock"))
}

/// Hold this instance's lock in `dir` for the rest of the process (once per folder).
fn claim_dir(dir: &Path) {
    static HELD: Mutex<Vec<(PathBuf, File)>> = Mutex::new(Vec::new());
    let Ok(mut held) = HELD.lock() else { return };
    if held.iter().any(|(d, _)| d == dir) {
        return;
    }
    let path = lock_path(dir, instance_id());
    match File::create(&path).and_then(|f| f.try_lock().map(|()| f).map_err(std::io::Error::other)) {
        Ok(f) => held.push((dir.to_path_buf(), f)),
        Err(e) => log::warn!("recovery: could not lock {}: {e}", path.display()),
    }
}

/// Whether another running instance holds `owner`'s lock in `dir`.
fn owner_alive(dir: &Path, owner: u64) -> bool {
    let Ok(f) = File::open(lock_path(dir, owner)) else { return false };
    matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

/// Write `bytes` to `path` through a temporary file and a rename, so a crash never leaves it torn.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))
}

/// Presentations reopened per start; the rest stay on disk for the next one, so a folder
/// full of entries can't stall start-up.
pub const MAX_REOPEN: usize = 32;
/// Largest entry read back.
const MAX_ENTRY_BYTES: u64 = 2 << 30;
const MAX_META_BYTES: u64 = 64 << 10;

/// `p` as bytes when it's a regular file no bigger than `max`: reading a FIFO left where an
/// entry belongs would block start-up forever.
fn read_regular(p: &Path, max: u64) -> Option<Vec<u8>> {
    let m = std::fs::metadata(p).ok()?;
    (m.is_file() && m.len() <= max).then(|| std::fs::read(p).ok()).flatten()
}

fn files(dir: &Path, uid: u64) -> (PathBuf, PathBuf) {
    (dir.join(format!("{uid}.deckcraft")), dir.join(format!("{uid}.json")))
}

fn now_secs() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// Remove a presentation's recovery entry (it was saved or closed).
pub fn discard(dir: &Path, uid: u64) {
    let (d, m) = files(dir, uid);
    let _ = std::fs::remove_file(d);
    let _ = std::fs::remove_file(m);
}

/// Write entries for the unsaved presentations (and drop those of saved ones). Returns how many
/// were written.
pub fn save(s: &Session, dir: &Path) -> Result<usize> {
    save_within(s, dir, MAX_ENTRY_BYTES)
}

/// Apply the recovery reader's encoded-file limit before replacing either checkpoint member.
fn save_within(s: &Session, dir: &Path, max_bytes: u64) -> Result<usize> {
    std::fs::create_dir_all(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    claim_dir(dir);
    let mut n = 0;
    for d in s.documents() {
        if !d.is_dirty() {
            discard(dir, d.uid);
            continue;
        }
        let (doc, meta) = files(dir, d.uid);
        let bytes = deckcraft_format::save(&d.doc).map_err(|e| EngineError::Other(e.to_string()))?;
        if bytes.len() as u64 > max_bytes {
            return Err(EngineError::Other("recovery snapshot is larger than DeckCraft can reopen".into()));
        }
        let metadata = json!({"path": d.path, "title": d.title(), "saved": now_secs(), "owner": instance_id()});
        write_checkpoint(&doc, &bytes, &meta, metadata, write_atomic)?;
        n += 1;
    }
    Ok(n)
}

/// Detach any previous checkpoint before replacement; publish a destination only for new bytes.
/// Both metadata forms must fit the reader's limit before the first write. The writer argument
/// lets tests refuse each replacement independently without filesystem-permission assumptions.
fn write_checkpoint(doc: &Path, bytes: &[u8], meta: &Path, mut metadata: Value, mut write: impl FnMut(&Path, &[u8]) -> Result<()>) -> Result<()> {
    let committed = metadata.to_string();
    if let Some(object) = metadata.as_object_mut() {
        object.insert("path".into(), Value::Null);
    }
    let conservative = metadata.to_string();
    if committed.len() as u64 > MAX_META_BYTES || conservative.len() as u64 > MAX_META_BYTES {
        return Err(EngineError::Other("recovery metadata is larger than DeckCraft can reopen".into()));
    }
    // Re-encoding missing payloads can remove damage warnings. Conversely, Save As can leave an
    // old checkpoint behind if cleanup fails. Neither old nor partial bytes may gain a new path.
    write(meta, conservative.as_bytes())?;
    write(doc, bytes)?;
    if committed != conservative {
        write(meta, committed.as_bytes())?;
    }
    Ok(())
}

/// Entries in `dir`: (uid, metadata).
pub fn list(dir: &Path) -> Vec<(u64, Value)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("deckcraft") {
            continue;
        }
        if !std::fs::metadata(&p).is_ok_and(|m| m.is_file()) {
            continue;
        }
        let Some(uid) = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u64>().ok()) else { continue };
        // Metadata is a file on disk: anything but an object (corrupt, hand-edited) counts as none.
        let meta = read_regular(&p.with_extension("json"), MAX_META_BYTES)
            .and_then(|t| serde_json::from_slice::<Value>(&t).ok())
            .filter(Value::is_object)
            .unwrap_or(json!({}));
        out.push((uid, meta));
    }
    out.sort_by_key(|e| e.0);
    out
}

/// Reopen every entry left by an instance that is no longer running as an unsaved presentation.
/// Entries stay on disk, now owned by this instance: the presentation keeps the entry's uid, so
/// the next recovery save replaces the snapshot and saving or closing it removes it. Entries of a
/// running instance, unreadable entries and entries already open here are left alone. Returns the
/// opened documents' indices. At most [`MAX_REOPEN`] are opened per start, the newest first; the rest
/// wait for the next one.
/// Missing media/chart parts are reported as damage and the recovered copy needs an explicit save
/// destination, so an ordinary Save cannot silently replace the original with partial content.
pub fn open(s: &mut Session, dir: &Path) -> Result<Vec<usize>> {
    let me = instance_id();
    let mut opened = Vec::new();
    let mut entries = list(dir);
    entries.sort_by_key(|(uid, meta)| std::cmp::Reverse((meta.get("saved").and_then(Value::as_u64).unwrap_or(0), *uid)));
    for (uid, mut meta) in entries {
        if opened.len() >= MAX_REOPEN {
            break;
        }
        if s.documents().iter().any(|d| d.uid == uid) {
            continue;
        }
        if let Some(owner) = meta.get("owner").and_then(Value::as_u64)
            && owner != me
            && owner_alive(dir, owner)
        {
            continue;
        }
        let (doc, meta_path) = files(dir, uid);
        let Some(bytes) = read_regular(&doc, MAX_ENTRY_BYTES) else { continue };
        let Ok((mut d, media)) = deckcraft_format::load_checked(&bytes) else { continue };
        deckcraft_format::repair(&mut d);
        let damaged = crate::cmd::file::Damage { media, slides: vec![] };
        // As with an ordinary damaged-file open, Save must ask for a new name instead of
        // replacing the original with the empty media or missing retained chart parts.
        let path = if damaged.is_empty() { meta.get("path").and_then(Value::as_str).map(str::to_string) } else { None };
        let title = meta.get("title").and_then(Value::as_str).filter(|t| !t.is_empty()).unwrap_or("Presentation").to_string();
        // Take the entry over (so another instance starting now leaves it alone), keeping its
        // snapshot until this one writes a newer one.
        claim_dir(dir);
        if let Some(o) = meta.as_object_mut() {
            o.insert("owner".into(), json!(me));
        }
        if let Err(e) = write_atomic(&meta_path, meta.to_string().as_bytes()) {
            log::warn!("recovery: {e}");
        }
        let mut st = DocState::new(d, path, format!("{title} (Recovered)"));
        st.uid = uid;
        // Unsaved: the copy on disk (if any) is older than what was recovered.
        st.saved_doc = std::sync::Arc::new((*st.doc).clone());
        st.revision += 1;
        opened.push(s.add_document(st));
        crate::cmd::file::report_damage(s, &damaged)?;
    }
    remove_stale_locks(dir);
    Ok(opened)
}

/// Delete the lock files of instances that are no longer running.
fn remove_stale_locks(dir: &Path) {
    let me = instance_id();
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(owner) =
            name.to_str().and_then(|n| n.strip_prefix("owner-")).and_then(|n| n.strip_suffix(".lock")).and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };
        if owner != me && !owner_alive(dir, owner) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deckcraft-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn recovers_unsaved_presentations() {
        let dir = temp("recovery");
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        // Only the edited presentation is written.
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        assert_eq!(save(&s, &dir).unwrap(), 1);
        assert_eq!(list(&dir).len(), 1);
        // "Crash": a new session reopens it, unsaved, with its slides.
        let mut s2 = Session::new();
        assert_eq!(open(&mut s2, &dir).unwrap().len(), 1);
        let st = s2.doc().unwrap();
        assert!(st.is_dirty());
        assert_eq!(st.doc.slides.len(), 2);
        assert!(st.title().ends_with("(Recovered)"));
        assert_eq!(list(&dir).len(), 1, "the snapshot stays until it is replaced or saved");
        // A second "crash" before any checkpoint: the presentation is recovered again.
        let mut s3 = Session::new();
        assert_eq!(open(&mut s3, &dir).unwrap().len(), 1);
        assert_eq!(s3.doc().unwrap().doc.slides.len(), 2);
        // Opening again in the same session doesn't duplicate it.
        assert!(open(&mut s3, &dir).unwrap().is_empty());
        // Saving the recovered presentation drops the entry.
        s3.recovery_dir = Some(dir.clone());
        let path = dir.join("saved.deckcraft");
        s3.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
        assert!(list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refused native serialization must leave the last readable checkpoint and metadata intact.
    #[test]
    fn rejected_snapshot_preserves_the_previous_recovery_entry() {
        use deckcraft_model::chart::{Chart, ChartKept, ChartPart, ChartType};
        use deckcraft_model::{Shape, ShapeId, ShapeKind};
        use std::sync::Arc;

        let dir = temp("recovery-rejected-snapshot");
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        save(&s, &dir).unwrap();
        let (doc_path, meta_path) = files(&dir, s.doc().unwrap().uid);
        let previous_doc = std::fs::read(&doc_path).unwrap();
        let previous_meta = std::fs::read(&meta_path).unwrap();

        let mut chart = Chart::sample(ChartType::Pie);
        chart.kept.parts = vec![
            ChartPart {
                name: "ppt/embeddings/owned.bin".into(),
                content_type: "application/octet-stream".into(),
                bytes: vec![],
                native_entry: None,
            };
            ChartKept::MAX_PARTS + 1
        ];
        let presentation = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        let id = ShapeId(presentation.alloc_id().unwrap());
        Arc::make_mut(&mut presentation.slides[0]).shapes.push(Shape { id, kind: ShapeKind::Chart(Box::new(chart)), ..Default::default() });
        assert!(save(&s, &dir).unwrap_err().to_string().contains("per-chart limits"));
        assert_eq!(std::fs::read(&doc_path).unwrap(), previous_doc);
        assert_eq!(std::fs::read(&meta_path).unwrap(), previous_meta);
        assert_eq!(deckcraft_format::load(&previous_doc).unwrap().slides.len(), 2);
        let mut reopened = Session::new();
        assert_eq!(open(&mut reopened, &dir).unwrap().len(), 1);
        assert_eq!(reopened.doc().unwrap().doc.slides.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Even a valid native archive must fit the recovery reader's separate encoded-file cap.
    #[test]
    fn oversized_recovery_members_leave_the_previous_checkpoint_intact() {
        let dir = temp("recovery-member-limits");
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        save(&s, &dir).unwrap();
        let (doc_path, meta_path) = files(&dir, s.doc().unwrap().uid);
        let previous_doc = std::fs::read(&doc_path).unwrap();
        let previous_meta = std::fs::read(&meta_path).unwrap();
        assert!(save_within(&s, &dir, 1).unwrap_err().to_string().contains("snapshot"));
        assert_eq!(std::fs::read(&doc_path).unwrap(), previous_doc);
        assert_eq!(std::fs::read(&meta_path).unwrap(), previous_meta);
        // This is small enough for the native format but too large for recovery metadata.
        s.doc_mut().unwrap().title = "owned".repeat(MAX_META_BYTES as usize / 5 + 1);
        assert!(save(&s, &dir).unwrap_err().to_string().contains("metadata"));
        assert_eq!(std::fs::read(&doc_path).unwrap(), previous_doc);
        assert_eq!(std::fs::read(&meta_path).unwrap(), previous_meta);
        assert_eq!(deckcraft_format::load(&previous_doc).unwrap().slides.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stale checkpoint left behind by failed Save As cleanup must never gain the new path
    /// before its document bytes have been replaced. Refuse every replacement, then restart.
    #[test]
    fn checkpoint_failures_do_not_publish_new_destinations_for_old_content() {
        for failed_write in 1..=4 {
            let root = temp(&format!("recovery-publish-path-{failed_write}"));
            std::fs::create_dir_all(&root).unwrap();
            let dir = root.join("recovery");
            let old_path = root.join("old.deckcraft");
            let new_path = root.join("new.deckcraft");
            let mut run = Session::with_new();
            run.recovery_dir = Some(dir.clone());
            run.execute("file.saveAs", &json!({"path": old_path.to_string_lossy()})).unwrap();
            run.execute("slide.new", &json!({"layout": "blank"})).unwrap();
            save(&run, &dir).unwrap();
            let uid = run.doc().unwrap().uid;
            let (doc_path, meta_path) = files(&dir, uid);
            let previous_doc = std::fs::read(&doc_path).unwrap();
            let previous_meta = std::fs::read(&meta_path).unwrap();
            run.execute("slide.new", &json!({"layout": "blank"})).unwrap();
            run.execute("file.saveAs", &json!({"path": new_path.to_string_lossy()})).unwrap();
            // Restore the observable result of failed cleanup: a two-slide checkpoint still
            // exists, while Save As has successfully saved three slides at the new destination.
            std::fs::write(&doc_path, &previous_doc).unwrap();
            std::fs::write(&meta_path, &previous_meta).unwrap();
            let old_file = std::fs::read(&old_path).unwrap();
            let new_file = std::fs::read(&new_path).unwrap();
            run.execute("slide.new", &json!({"layout": "blank"})).unwrap();
            let next_doc = deckcraft_format::save(&run.doc().unwrap().doc).unwrap();
            let next_meta = json!({"path": run.doc().unwrap().path, "title": "new", "saved": now_secs(), "owner": instance_id()});
            let mut writes = 0;
            let result = write_checkpoint(&doc_path, &next_doc, &meta_path, next_meta, |path, bytes| {
                writes += 1;
                if writes == failed_write {
                    return Err(EngineError::Other("injected checkpoint replacement failure".into()));
                }
                write_atomic(path, bytes)
            });
            assert_eq!(result.is_err(), failed_write <= 3);
            assert_eq!(std::fs::read(&old_path).unwrap(), old_file);
            assert_eq!(std::fs::read(&new_path).unwrap(), new_file);
            drop(run);

            let mut recovered = Session::new();
            recovered.recovery_dir = Some(dir.clone());
            assert_eq!(open(&mut recovered, &dir).unwrap(), vec![0]);
            let st = recovered.doc().unwrap();
            assert!(st.is_dirty());
            assert_eq!(st.doc.slides.len(), if failed_write <= 2 { 2 } else { 4 });
            if failed_write == 1 {
                assert_eq!(st.path.as_deref(), old_path.to_str(), "the first refused write leaves the old pair intact");
            } else if failed_write <= 3 {
                assert!(st.path.is_none(), "incomplete checkpoint replacement cannot publish the new destination");
            } else {
                assert_eq!(st.path.as_deref(), new_path.to_str(), "a completed checkpoint publishes its matching destination");
            }
            let saved = recovered.execute("file.save", &json!({})).unwrap();
            if matches!(failed_write, 2 | 3) {
                assert_eq!(saved, json!({"needsPath": true}));
            }
            let saved_bytes = std::fs::read(&new_path).unwrap();
            assert_eq!(deckcraft_format::load(&saved_bytes).unwrap().slides.len(), if failed_write <= 3 { 3 } else { 4 });
            if failed_write <= 3 {
                assert_eq!(saved_bytes, new_file, "a failed checkpoint cannot cause Save to overwrite the newer file");
            }
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// Null is two bytes longer than an empty path: validate the conservative metadata too.
    #[test]
    fn both_checkpoint_metadata_forms_are_bounded_before_any_write() {
        let committed = json!({"path": "", "title": ""}).to_string();
        let padding = MAX_META_BYTES as usize - committed.len();
        let metadata = json!({"path": "", "title": "x".repeat(padding)});
        assert_eq!(metadata.to_string().len() as u64, MAX_META_BYTES);
        let result = write_checkpoint(Path::new("unused.deckcraft"), b"unused", Path::new("unused.json"), metadata, |_, _| {
            panic!("oversized conservative metadata must be refused before any write")
        });
        assert!(result.unwrap_err().to_string().contains("metadata"));
    }

    #[test]
    fn uids_are_unique_beyond_this_process() {
        let a = DocState::new(Default::default(), None, "a".into());
        let b = DocState::new(Default::default(), None, "b".into());
        assert_ne!(a.uid, b.uid);
        // Not the small per-process numbers every instance used to start from.
        assert!(a.uid >= 1 << 24 && a.uid <= UID_MASK);
    }

    #[test]
    fn another_running_instance_keeps_its_entries() {
        let dir = temp("recovery-shared");
        std::fs::create_dir_all(&dir).unwrap();
        // Another instance (its own owner id and uid, its lock held) has an unsaved presentation.
        let other = instance_id() ^ (1 << 40);
        let lock = File::create(lock_path(&dir, other)).unwrap();
        lock.try_lock().unwrap();
        let mut o = Session::new();
        o.execute("file.new", &json!({})).unwrap();
        o.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let bytes = deckcraft_format::save(&o.doc().unwrap().doc).unwrap();
        let uid = other + 5;
        let (d, m) = files(&dir, uid);
        std::fs::write(&d, bytes).unwrap();
        std::fs::write(&m, json!({"title": "Theirs", "owner": other}).to_string()).unwrap();
        // This instance saving its clean and dirty presentations leaves that entry alone...
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        assert_eq!(save(&s, &dir).unwrap(), 1);
        assert!(list(&dir).iter().any(|e| e.0 == uid));
        // ...and a recovery open here skips it while its owner runs.
        let mut s2 = Session::new();
        let opened = open(&mut s2, &dir).unwrap();
        assert!(s2.documents().iter().all(|d| d.uid != uid), "{opened:?}");
        assert!(lock_path(&dir, other).exists(), "a live owner's lock stays");
        // Once the owner is gone, its entry is recovered and its lock file cleaned up. Unlock
        // rather than only drop: a child spawned meanwhile by a parallel test (mkfifo) shares the
        // open file and would keep the lock held until it execs.
        lock.unlock().unwrap();
        drop(lock);
        let mut s3 = Session::new();
        open(&mut s3, &dir).unwrap();
        assert!(s3.documents().iter().any(|d| d.uid == uid && d.title().starts_with("Theirs")));
        assert!(!lock_path(&dir, other).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_entries_are_skipped() {
        let dir = temp("recovery-corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("7.deckcraft"), b"not a presentation").unwrap();
        std::fs::write(dir.join("7.json"), b"[1, 2]").unwrap();
        let mut s = Session::new();
        assert!(open(&mut s, &dir).unwrap().is_empty());
        s.recovery_dir = Some(dir.clone());
        let v = s.execute("file.recovery.list", &json!({})).unwrap();
        assert_eq!(v.as_array().map(Vec::len), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A FIFO where an entry or its metadata belongs is skipped rather than read (that blocked
    /// start-up forever), and a folder full of entries reopens only [`MAX_REOPEN`] per start.
    #[cfg(unix)]
    #[test]
    fn hostile_recovery_folders_dont_stall_start_up() {
        let dir = temp("recovery-hostile");
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        save(&s, &dir).unwrap();
        let (good, _) = files(&dir, s.doc().unwrap().uid);
        for i in 1..=(MAX_REOPEN as u64 + 3) {
            std::fs::copy(&good, dir.join(format!("{}.deckcraft", 100_000 + i))).unwrap();
        }
        for name in ["5.deckcraft", "6.json"] {
            let _ = std::process::Command::new("mkfifo").arg(dir.join(name)).status();
        }
        std::fs::write(dir.join("6.deckcraft"), std::fs::read(&good).unwrap()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let d = dir.clone();
        std::thread::spawn(move || {
            let mut s2 = Session::new();
            let _ = tx.send(open(&mut s2, &d).map(|v| v.len()).unwrap_or(0));
        });
        let n = rx.recv_timeout(std::time::Duration::from_secs(60)).expect("recovery hung");
        assert_eq!(n, MAX_REOPEN);
        // Every entry stays on disk (main keeps snapshots until a newer save); the 32 opened plus
        // the rest that wait for the next start.
        assert_eq!(list(&dir).len(), MAX_REOPEN + 5, "the rest wait for the next start");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
