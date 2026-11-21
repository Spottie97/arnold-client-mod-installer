//! Destination changes and installs, shared by the Tauri commands. No Tauri types.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::detect::ProcessInfo;
use crate::errors::AppError;
use crate::manifest::Manifest;
use crate::pack_dirs::{self, DestinationEntry};
use crate::packs;
use crate::plan::{build_plan, ensure_plan_current, InstallPlan, Selection};
use crate::prefs::{PackLocation, Prefs};
use crate::state::{load_state, InstalledState};

pub struct Ctx<'a> {
    pub launcher_root: &'a Path,
    pub search_roots: &'a [PathBuf],
    pub processes: &'a [ProcessInfo],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderChoice {
    Keep,
    Fresh,
}

impl FolderChoice {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "keep" => Some(Self::Keep),
            "fresh" => Some(Self::Fresh),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Change {
    SelectPack(String),
    SetGameDir(PathBuf),
    UseDefaultFolder(bool),
    ChooseFolder(FolderChoice),
    SwitchShared(String),
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub prefs: Prefs,
    pub selected: String,
    pub reselect: bool,
    pub game_dir: Option<PathBuf>,
    pub entry: Option<DestinationEntry>,
}

#[derive(Debug, Clone)]
pub struct InstallOutcome {
    pub run_id: String,
    pub entry: DestinationEntry,
}

/// The only way a destination-changing command may compute new prefs.
/// Never saves prefs and never touches app state.
pub fn apply_change(
    prefs: &Prefs,
    selected: &str,
    ctx: &Ctx,
    change: Change,
    load_manifest: impl FnOnce(&str) -> Result<Manifest, AppError>,
) -> Result<Outcome, AppError> {
    match change {
        Change::SelectPack(id) => {
            let mut next = prefs.clone();
            next.selected_pack = Some(id.clone());
            let entry = enter(&next, &id, ctx.launcher_root, ctx, load_manifest)?;
            Ok(Outcome {
                prefs: next,
                selected: id,
                reselect: true,
                game_dir: None,
                entry: Some(entry),
            })
        }
        Change::SetGameDir(path) => {
            let mut next = prefs.clone();
            next.last_game_dir = Some(path.clone());
            let entry = enter(&next, selected, &path, ctx, load_manifest)?;
            Ok(Outcome {
                prefs: next,
                selected: selected.to_string(),
                reselect: false,
                game_dir: Some(path),
                entry: Some(entry),
            })
        }
        Change::UseDefaultFolder(enabled) => {
            let mut next = prefs.clone();
            next.use_default_folder = enabled;
            let entry = if enabled {
                Some(enter(
                    &next,
                    selected,
                    ctx.launcher_root,
                    ctx,
                    load_manifest,
                )?)
            } else {
                None
            };
            Ok(Outcome {
                prefs: next,
                selected: selected.to_string(),
                reselect: false,
                game_dir: None,
                entry,
            })
        }
        Change::ChooseFolder(FolderChoice::Keep) => {
            let legacy =
                pack_dirs::resolve_pack_dir(prefs, selected, ctx.launcher_root, ctx.search_roots)
                    .needs_choice
                    .map(|choice| choice.root);
            let root = legacy.unwrap_or_else(|| ctx.launcher_root.to_path_buf());
            let mut next = prefs.clone();
            next.pack_locations
                .insert(selected.to_string(), PackLocation::DefaultFolder);
            next.last_game_dir = Some(root.clone());
            let entry = enter(&next, selected, &root, ctx, load_manifest)?;
            Ok(Outcome {
                prefs: next,
                selected: selected.to_string(),
                reselect: false,
                game_dir: Some(root),
                entry: Some(entry),
            })
        }
        Change::ChooseFolder(FolderChoice::Fresh) => {
            let legacy =
                pack_dirs::resolve_pack_dir(prefs, selected, ctx.launcher_root, ctx.search_roots)
                    .needs_choice
                    .map(|choice| choice.root);
            let mut next = prefs.clone();
            next.pack_locations
                .insert(selected.to_string(), PackLocation::OwnFolder);
            let mut game_dir = None;
            if let Some(root) = legacy.filter(|_| next.last_game_dir.is_none()) {
                next.last_game_dir = Some(root.clone());
                game_dir = Some(root);
            }
            Ok(Outcome {
                prefs: next,
                selected: selected.to_string(),
                reselect: false,
                game_dir,
                entry: None,
            })
        }
        Change::SwitchShared(to_id) => {
            let resolved =
                pack_dirs::resolve_pack_dir(prefs, selected, ctx.launcher_root, ctx.search_roots);
            if resolved.needs_choice.is_some() || resolved.mode != "default" {
                return Err(AppError::State(
                    "pack switching only applies when packs share the Minecraft folder".into(),
                ));
            }
            let mut next = prefs.clone();
            next.selected_pack = Some(to_id.clone());
            let entry = enter(&next, &to_id, ctx.launcher_root, ctx, load_manifest)?;
            Ok(Outcome {
                prefs: next,
                selected: to_id,
                reselect: true,
                game_dir: None,
                entry: Some(entry),
            })
        }
    }
}

fn enter(
    prefs: &Prefs,
    pack_id: &str,
    launcher_root: &Path,
    ctx: &Ctx,
    load_manifest: impl FnOnce(&str) -> Result<Manifest, AppError>,
) -> Result<DestinationEntry, AppError> {
    pack_dirs::enter_destination(
        prefs,
        pack_id,
        launcher_root,
        ctx.search_roots,
        || load_manifest(pack_id),
        ctx.processes,
    )
}

/// Folder that writes may target.
pub fn owned_pack_dir(
    prefs: &Prefs,
    selected: &str,
    launcher_root: &Path,
    search_roots: &[PathBuf],
) -> Result<PathBuf, AppError> {
    let resolved = pack_dirs::resolve_pack_dir(prefs, selected, launcher_root, search_roots);
    if let Some(choice) = &resolved.needs_choice {
        return Err(packs::folder_choice_error(
            choice.root.display().to_string(),
        ));
    }
    if resolved.mode == "default" {
        pack_dirs::require_owner(&resolved.path, selected)?;
    }
    Ok(resolved.path)
}

/// Folder an explicit 2.3.x import may write state.json into. Read only.
/// Only Better Adventures+ may adopt a shared folder that has Arnold jars but no installer record.
pub fn import_dir_at(
    prefs: &Prefs,
    selected: &str,
    launcher_root: &Path,
    search_roots: &[PathBuf],
) -> Result<PathBuf, AppError> {
    let resolved = pack_dirs::resolve_pack_dir(prefs, selected, launcher_root, search_roots);
    let (dir, shared) = match resolved.needs_choice {
        Some(choice) => (choice.root, true),
        None => {
            let shared = resolved.mode == "default";
            (resolved.path, shared)
        }
    };
    if !shared {
        return Ok(dir);
    }
    match pack_dirs::resolve_folder_owner(&dir) {
        pack_dirs::FolderOwner::Occupied { files } => {
            if selected == packs::BA_PLUS_PACK_ID {
                Ok(dir)
            } else {
                Err(packs::folder_occupied_error(&dir, selected, &files))
            }
        }
        _ => Err(AppError::State(
            "there is nothing to adopt here: this folder already has an installer record, or no Arnold jars".into(),
        )),
    }
}

/// Folder reads and plans describe.
pub fn plan_dir_at(
    prefs: &Prefs,
    selected: &str,
    launcher_root: &Path,
    search_roots: &[PathBuf],
) -> PathBuf {
    let resolved = pack_dirs::resolve_pack_dir(prefs, selected, launcher_root, search_roots);
    if let Some(choice) = resolved.needs_choice {
        return choice.root;
    }
    resolved.path
}

/// Installed state that belongs to the selected pack, never another pack's live state.
pub fn incoming_state_at(
    prefs: &Prefs,
    selected: &str,
    launcher_root: &Path,
    search_roots: &[PathBuf],
) -> Result<Option<InstalledState>, AppError> {
    let resolved = pack_dirs::resolve_pack_dir(prefs, selected, launcher_root, search_roots);
    if let Some(choice) = &resolved.needs_choice {
        return pack_dirs::shared_state_for(&choice.root, selected);
    }
    if resolved.mode == "default" {
        return pack_dirs::shared_state_for(&resolved.path, selected);
    }
    load_state(&resolved.path)
}

/// Read-only plan for the selected pack's current folder.
pub fn plan_at(
    manifest: &Manifest,
    sel: &Selection,
    prefs: &Prefs,
    selected: &str,
    launcher_root: &Path,
    search_roots: &[PathBuf],
    fabric_installed: bool,
) -> Result<InstallPlan, AppError> {
    let installed = incoming_state_at(prefs, selected, launcher_root, search_roots)?;
    let mut plan = build_plan(manifest, sel, installed.as_ref(), fabric_installed)?;
    plan.pack_id = selected.to_string();
    plan.pack_dir = plan_dir_at(prefs, selected, launcher_root, search_roots)
        .to_string_lossy()
        .into_owned();
    Ok(plan)
}

/// Ownership backstop used by install and repair.
pub fn prepare_destination(
    prefs: &Prefs,
    selected: &str,
    ctx: &Ctx,
    load_manifest: impl FnOnce(&str) -> Result<Manifest, AppError>,
) -> Result<(DestinationEntry, PathBuf), AppError> {
    let entry = enter(prefs, selected, ctx.launcher_root, ctx, load_manifest)?;
    let dir = owned_pack_dir(prefs, selected, ctx.launcher_root, ctx.search_roots)?;
    Ok((entry, dir))
}

pub fn install_at(
    plan: &InstallPlan,
    prefs: &Prefs,
    selected: &str,
    ctx: &Ctx,
    manifest: &Manifest,
    load_manifest: impl FnOnce(&str) -> Result<Manifest, AppError>,
    run: impl FnOnce(&Path) -> Result<String, AppError>,
) -> Result<InstallOutcome, AppError> {
    let (entry, dir) = prepare_destination(prefs, selected, ctx, load_manifest)?;
    let installed = load_state(&dir).ok().flatten();
    ensure_plan_current(plan, selected, &dir, manifest, installed.as_ref())?;
    let run_id = run(&dir)?;
    record_active_pack_if_shared(prefs, selected, ctx.launcher_root, ctx.search_roots)?;
    Ok(InstallOutcome { run_id, entry })
}

pub fn record_active_pack_if_shared(
    prefs: &Prefs,
    selected: &str,
    launcher_root: &Path,
    search_roots: &[PathBuf],
) -> Result<(), AppError> {
    let resolved = pack_dirs::resolve_pack_dir(prefs, selected, launcher_root, search_roots);
    if resolved.needs_choice.is_none() && resolved.mode == "default" {
        pack_dirs::write_active_pack(
            &resolved.path,
            selected,
            None,
            pack_dirs::OwnerSource::Marker,
        )?;
    }
    Ok(())
}

/// Appends the pack-folder-switch line when something happened.
/// Returns true when the UI should be told.
pub fn record_entry(entry: &DestinationEntry) -> bool {
    if entry.switch.is_none() && !entry.adopted {
        return false;
    }
    let _ = crate::log::append_install_log(
        &entry.path,
        &json!({
            "event": "pack-folder-switch",
            "packId": entry.pack_id,
            "path": entry.path,
            "ownerBefore": entry.owner_before,
            "switch": entry.switch,
            "adopted": entry.adopted,
        }),
    );
    true
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    use tempfile::tempdir;

    use super::{
        apply_change, import_dir_at, incoming_state_at, install_at, plan_at, AppError, Change, Ctx,
        FolderChoice, InstallPlan, Manifest, PackLocation, Prefs,
    };
    use crate::backup::{list_snapshots, snapshots_dir};
    use crate::download::sha512_hex;
    use crate::fetch::MapFetcher;
    use crate::import::adopt_import;
    use crate::install::test_support::{
        basic_selection, legacy_shared_game, prepare_game, test_manifest_and_fetcher,
    };
    use crate::install::{dest_rel, run_install, InstallTarget};
    use crate::pack_dirs::{
        kept_aside_paths, read_active_pack, write_active_pack, FolderOwner, OwnerSource,
    };
    use crate::packs::BA_PLUS_PACK_ID;
    use crate::plan::{build_plan, ensure_plan_current};
    use crate::state::{load_state, record_file, save_state, state_path, InstalledState};

    fn ctx(root: &Path) -> Ctx<'_> {
        Ctx {
            launcher_root: root,
            search_roots: &[],
            processes: &[],
        }
    }

    fn no_loader(_: &str) -> Result<Manifest, AppError> {
        panic!("manifest loader must not run");
    }

    fn loader(manifest: &Manifest) -> impl FnOnce(&str) -> Result<Manifest, AppError> + '_ {
        |_: &str| Ok(manifest.clone())
    }

    fn run_with<'a>(
        plan: &'a InstallPlan,
        manifest: &'a Manifest,
        fetcher: &'a MapFetcher,
        replace: bool,
    ) -> impl FnOnce(&Path) -> Result<String, AppError> + 'a {
        move |dir| {
            run_install(
                &InstallTarget::same_folder(dir),
                plan,
                manifest,
                fetcher,
                &AtomicBool::new(false),
                replace,
                16.0,
                &|_| {},
            )
        }
    }

    fn shared_prefs(pack_id: &str) -> Prefs {
        Prefs {
            selected_pack: Some(pack_id.to_string()),
            use_default_folder: true,
            ..Prefs::default()
        }
    }

    fn keep_prefs(pack_id: &str) -> Prefs {
        let mut prefs = Prefs {
            selected_pack: Some(pack_id.to_string()),
            use_default_folder: false,
            ..Prefs::default()
        };
        prefs
            .pack_locations
            .insert(pack_id.to_string(), PackLocation::DefaultFolder);
        prefs
    }

    fn tree_fingerprint(root: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
            let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if path.is_dir() {
                    out.push((format!("{rel}/"), String::new()));
                    walk(root, &path, out);
                } else {
                    out.push((rel, sha512_hex(&fs::read(&path).unwrap())));
                }
            }
        }
        if root.exists() {
            walk(root, root, &mut out);
        }
        out
    }

    fn mark_pack_a(game: &Path, bytes: &[u8]) {
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::write(game.join("mods/a-only.jar"), bytes).unwrap();
        let mut state = InstalledState::new("installer", "a-1");
        state.pack_id = Some("pack-a".into());
        record_file(&mut state, "mods/a-only.jar", sha512_hex(bytes), "pack-a");
        save_state(game, &state).unwrap();
        write_active_pack(game, "pack-a", None, OwnerSource::Marker).unwrap();
    }

    fn stash_real_install(
        scratch: &Path,
        game: &Path,
        pack_id: &str,
        manifest: &Manifest,
        fetcher: &MapFetcher,
    ) -> InstalledState {
        prepare_game(scratch);
        let mut scratch_plan = build_plan(manifest, &basic_selection(), None, true).unwrap();
        scratch_plan.pack_id = pack_id.to_string();
        scratch_plan.pack_dir = scratch.to_string_lossy().into_owned();
        run_install(
            &InstallTarget::same_folder(scratch),
            &scratch_plan,
            manifest,
            fetcher,
            &AtomicBool::new(false),
            false,
            16.0,
            &|_| {},
        )
        .unwrap();
        let scratch_state = load_state(scratch).unwrap().unwrap();
        let stash = game.join("arnold-installer/packs").join(pack_id);
        for file in &scratch_state.files {
            let from = scratch.join(&file.path);
            let to = stash.join(&file.path);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::copy(&from, &to).unwrap();
        }
        let state_dest = stash.join("arnold-installer/state.json");
        fs::create_dir_all(state_dest.parent().unwrap()).unwrap();
        fs::copy(state_path(scratch), &state_dest).unwrap();
        scratch_state
    }

    fn install_pack_x_over_legacy(
        game: &Path,
        prefs: &Prefs,
        manifest: &Manifest,
        fetcher: &MapFetcher,
        ba: &[u8],
        snow: &[u8],
        options_before_transition: &[u8],
    ) {
        // The folder transition itself must not touch the player's options.txt.
        assert_eq!(
            fs::read(game.join("options.txt")).unwrap(),
            options_before_transition
        );
        let plan = plan_at(
            manifest,
            &basic_selection(),
            prefs,
            "pack-x",
            game,
            &[],
            true,
        )
        .unwrap();
        assert_eq!(plan.pack_dir, game.to_string_lossy().into_owned());
        assert!(
            plan.remove.is_empty(),
            "plan used the outgoing pack's removals"
        );
        assert!(
            plan.update.is_empty(),
            "plan used the outgoing pack's updates"
        );
        install_at(
            &plan,
            prefs,
            "pack-x",
            &ctx(game),
            manifest,
            no_loader,
            run_with(&plan, manifest, fetcher, false),
        )
        .unwrap();
        let stash = game.join("arnold-installer/packs").join(BA_PLUS_PACK_ID);
        assert_eq!(fs::read(stash.join("mods/ba.jar")).unwrap(), ba);
        assert_eq!(
            fs::read(stash.join("mods/arnold-snowfall-1.3.3.jar")).unwrap(),
            snow
        );
        assert_eq!(
            load_state(&stash).unwrap().unwrap().manifest_version,
            "ba-plus-1"
        );
        assert_eq!(
            load_state(game).unwrap().unwrap().pack_id.as_deref(),
            Some("pack-x")
        );
        assert_eq!(read_active_pack(game).as_deref(), Some("pack-x"));
        assert_eq!(fs::read(game.join("mods/user.jar")).unwrap(), b"user-jar");
        assert_eq!(
            fs::read(game.join("saves/world/level.dat")).unwrap(),
            b"world"
        );
        assert_eq!(
            fs::read(game.join("screenshots/shot.png")).unwrap(),
            b"shot"
        );
        assert_player_options_kept(
            options_before_transition,
            &fs::read(game.join("options.txt")).unwrap(),
        );
        assert!(
            !load_state(game)
                .unwrap()
                .unwrap()
                .files
                .iter()
                .any(|file| file.path == "options.txt"),
            "options.txt is the player's file and is never owned"
        );
    }

    /// Quality may only append vanilla keys the player has not set. Every byte
    /// the player wrote stays, and a file that already sets them is unchanged.
    fn assert_player_options_kept(before: &[u8], after: &[u8]) {
        const QUALITY_KEYS: [&str; 2] = ["renderDistance", "cloudStatus"];
        let before = std::str::from_utf8(before).unwrap();
        let after = std::str::from_utf8(after).unwrap();
        let has_key = |text: &str, key: &str| {
            text.lines()
                .any(|line| line.split_once(':').is_some_and(|(k, _)| k == key))
        };
        if QUALITY_KEYS.iter().all(|key| has_key(before, key)) {
            assert_eq!(after, before, "a complete options.txt must not change");
            return;
        }
        assert!(
            after.starts_with(before),
            "player's options.txt text was rewritten: {after:?}"
        );
        for line in after[before.len()..].lines().filter(|l| !l.is_empty()) {
            let key = line.split_once(':').map(|(k, _)| k).unwrap_or(line);
            assert!(
                QUALITY_KEYS.contains(&key) && !has_key(before, key),
                "unexpected options.txt change: {line:?}"
            );
        }
    }

    fn occupied(result: Result<super::Outcome, AppError>) {
        assert_eq!(result.unwrap_err().code(), "E-FOLDER-OCCUPIED");
    }

    fn first_mods_rel(manifest: &Manifest) -> String {
        let plan = build_plan(manifest, &basic_selection(), None, true).unwrap();
        plan.add
            .iter()
            .map(dest_rel)
            .find(|rel| rel.starts_with("mods/"))
            .expect("basic plan adds a mod")
    }

    fn retarget(manifest: &mut Manifest, rel: &str, bytes: &[u8]) -> String {
        for component in &mut manifest.components {
            for file in &mut component.files {
                if format!("{}/{}", file.dest.folder(), file.file) == rel {
                    file.sha512 = sha512_hex(bytes);
                    file.size = bytes.len() as u64;
                    return file.url.clone();
                }
            }
        }
        panic!("plan file {rel} is not in the manifest");
    }

    #[test]
    fn legacy_folder_select_then_default_toggle_then_install() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join("game");
        prepare_game(&game);
        let (ba, snow) = legacy_shared_game(&game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let options = fs::read(game.join("options.txt")).unwrap();
        let before = tree_fingerprint(&game);
        let selected = apply_change(
            &Prefs::default(),
            BA_PLUS_PACK_ID,
            &ctx(&game),
            Change::SelectPack("pack-x".into()),
            no_loader,
        )
        .unwrap();
        assert_eq!(selected.selected, "pack-x");
        assert!(selected.reselect);
        assert_eq!(selected.entry.as_ref().unwrap().mode, "own");
        assert_eq!(tree_fingerprint(&game), before);
        let toggled = apply_change(
            &selected.prefs,
            "pack-x",
            &ctx(&game),
            Change::UseDefaultFolder(true),
            loader(&manifest),
        )
        .unwrap();
        let report = toggled.entry.as_ref().unwrap().switch.as_ref().unwrap();
        assert!(toggled.prefs.use_default_folder);
        assert!(report.legacy);
        assert_eq!(report.from, BA_PLUS_PACK_ID);
        install_pack_x_over_legacy(
            &game,
            &toggled.prefs,
            &manifest,
            &fetcher,
            &ba,
            &snow,
            &options,
        );
    }

    #[test]
    fn legacy_folder_select_then_keep_then_install() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join("game");
        prepare_game(&game);
        let (ba, snow) = legacy_shared_game(&game);
        // A real options.txt already sets the quality keys, so it must come
        // through the transition and the install byte for byte.
        let options =
            b"version:4671\r\nrenderDistance:20\r\ncloudStatus:false\r\nfov:0.25".to_vec();
        fs::write(game.join("options.txt"), &options).unwrap();
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let selected = apply_change(
            &Prefs::default(),
            BA_PLUS_PACK_ID,
            &ctx(&game),
            Change::SelectPack("pack-x".into()),
            no_loader,
        )
        .unwrap();
        let kept = apply_change(
            &selected.prefs,
            "pack-x",
            &ctx(&game),
            Change::ChooseFolder(FolderChoice::Keep),
            loader(&manifest),
        )
        .unwrap();
        assert_eq!(kept.game_dir.as_deref(), Some(game.as_path()));
        assert_eq!(
            kept.prefs.pack_locations.get("pack-x"),
            Some(&PackLocation::DefaultFolder)
        );
        assert_eq!(kept.prefs.last_game_dir.as_deref(), Some(game.as_path()));
        let report = kept.entry.as_ref().unwrap().switch.as_ref().unwrap();
        assert!(report.legacy);
        assert_eq!(report.from, BA_PLUS_PACK_ID);
        install_pack_x_over_legacy(
            &game,
            &kept.prefs,
            &manifest,
            &fetcher,
            &ba,
            &snow,
            &options,
        );
        assert_eq!(fs::read(game.join("options.txt")).unwrap(), options);
    }

    #[test]
    fn marked_folder_select_then_set_game_dir_then_install() {
        let tmp = tempdir().unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        let scratch = tmp.path().join("scratch");
        let game = tmp.path().join("game");
        prepare_game(&elsewhere);
        prepare_game(&game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let a_bytes = b"pack-a-only";
        mark_pack_a(&game, a_bytes);
        stash_real_install(&scratch, &game, "pack-x", &manifest, &fetcher);
        let starting = shared_prefs("pack-a");
        let selected = apply_change(
            &starting,
            "pack-a",
            &ctx(&elsewhere),
            Change::SelectPack("pack-x".into()),
            no_loader,
        )
        .unwrap();
        let moved = apply_change(
            &selected.prefs,
            "pack-x",
            &ctx(&elsewhere),
            Change::SetGameDir(game.clone()),
            loader(&manifest),
        )
        .unwrap();
        assert_eq!(moved.game_dir.as_deref(), Some(game.as_path()));
        assert_eq!(moved.prefs.last_game_dir.as_deref(), Some(game.as_path()));
        let report = moved.entry.as_ref().unwrap().switch.as_ref().unwrap();
        assert_eq!(report.from, "pack-a");
        assert!(report.restored);
        assert!(!report.legacy);
        let plan = plan_at(
            &manifest,
            &basic_selection(),
            &moved.prefs,
            "pack-x",
            &game,
            &[],
            true,
        )
        .unwrap();
        let installed = load_state(&game).unwrap().unwrap();
        for file in &installed.files {
            assert!(
                plan.add.iter().all(|item| dest_rel(item) != file.path),
                "{}",
                file.path
            );
        }
        install_at(
            &plan,
            &moved.prefs,
            "pack-x",
            &ctx(&game),
            &manifest,
            no_loader,
            run_with(&plan, &manifest, &fetcher, false),
        )
        .unwrap();
        assert_eq!(
            fs::read(game.join("arnold-installer/packs/pack-a/mods/a-only.jar")).unwrap(),
            a_bytes
        );
        assert_eq!(read_active_pack(&game).as_deref(), Some("pack-x"));
        let stash = game.join("arnold-installer/packs/pack-x");
        assert!(stash.is_dir());
        assert!(state_path(&stash).is_file());
        assert_eq!(
            load_state(&stash).unwrap().unwrap().pack_id.as_deref(),
            Some("pack-x")
        );
    }

    #[test]
    fn marked_folder_select_then_default_toggle_then_install() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join("game");
        prepare_game(&game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let a_bytes = b"pack-a-only";
        mark_pack_a(&game, a_bytes);
        let own = Prefs {
            selected_pack: Some("pack-a".into()),
            ..Prefs::default()
        };
        let selected = apply_change(
            &own,
            "pack-a",
            &ctx(&game),
            Change::SelectPack("pack-x".into()),
            no_loader,
        )
        .unwrap();
        let toggled = apply_change(
            &selected.prefs,
            "pack-x",
            &ctx(&game),
            Change::UseDefaultFolder(true),
            loader(&manifest),
        )
        .unwrap();
        let report = toggled.entry.as_ref().unwrap().switch.as_ref().unwrap();
        assert_eq!(report.from, "pack-a");
        assert!(!report.legacy);
        let plan = plan_at(
            &manifest,
            &basic_selection(),
            &toggled.prefs,
            "pack-x",
            &game,
            &[],
            true,
        )
        .unwrap();
        install_at(
            &plan,
            &toggled.prefs,
            "pack-x",
            &ctx(&game),
            &manifest,
            no_loader,
            run_with(&plan, &manifest, &fetcher, false),
        )
        .unwrap();
        assert_eq!(
            fs::read(game.join("arnold-installer/packs/pack-a/mods/a-only.jar")).unwrap(),
            a_bytes
        );
    }

    #[test]
    fn occupied_folder_refusals_change_nothing() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join("game");
        let elsewhere = tmp.path().join("elsewhere");
        prepare_game(&game);
        prepare_game(&elsewhere);
        fs::write(game.join("mods/arnold-snowfall-1.3.3.jar"), b"leftover").unwrap();
        fs::write(
            game.join("mods/arnold-snowfall-1.3.2.jar"),
            b"arbitrary-user-bytes",
        )
        .unwrap();
        let (manifest, _, _) = test_manifest_and_fetcher();
        let before = tree_fingerprint(&game);
        for pack in ["pack-x", BA_PLUS_PACK_ID] {
            let plain = Prefs {
                selected_pack: Some(pack.into()),
                ..Prefs::default()
            };
            occupied(apply_change(
                &plain,
                pack,
                &ctx(&game),
                Change::ChooseFolder(FolderChoice::Keep),
                no_loader,
            ));
            occupied(apply_change(
                &plain,
                pack,
                &ctx(&game),
                Change::UseDefaultFolder(true),
                no_loader,
            ));
            occupied(apply_change(
                &shared_prefs(pack),
                pack,
                &ctx(&elsewhere),
                Change::SetGameDir(game.clone()),
                no_loader,
            ));
            let plan = build_plan(&manifest, &basic_selection(), None, true).unwrap();
            let err = install_at(
                &plan,
                &keep_prefs(pack),
                pack,
                &ctx(&game),
                &manifest,
                no_loader,
                |_| panic!("install must not run"),
            )
            .unwrap_err();
            assert_eq!(err.code(), "E-FOLDER-OCCUPIED");
        }
        assert_eq!(tree_fingerprint(&game), before);
        assert!(!game.join("arnold-installer").exists());
        assert_eq!(
            fs::read(game.join("mods/arnold-snowfall-1.3.2.jar")).unwrap(),
            b"arbitrary-user-bytes"
        );
    }

    #[test]
    fn partial_restore_then_update_never_overwrites_the_users_file() {
        let tmp = tempdir().unwrap();
        let scratch = tmp.path().join("scratch");
        let game = tmp.path().join("game");
        prepare_game(&game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let rel = first_mods_rel(&manifest);
        let v1 = {
            let name = rel.rsplit('/').next().unwrap_or(&rel);
            format!("blob-{name}").into_bytes()
        };
        stash_real_install(&scratch, &game, "pack-x", &manifest, &fetcher);
        mark_pack_a(&game, b"pack-a-only");
        fs::write(game.join(&rel), b"players-own-file").unwrap();
        let switched = apply_change(
            &shared_prefs("pack-a"),
            "pack-a",
            &ctx(&game),
            Change::SelectPack("pack-x".into()),
            loader(&manifest),
        )
        .unwrap();
        let report = switched.entry.as_ref().unwrap().switch.as_ref().unwrap();
        assert!(report.partial_restore);
        assert_eq!(report.conflicts, vec![rel.clone()]);
        assert_eq!(kept_aside_paths(&game).unwrap(), vec![rel.clone()]);
        let live = incoming_state_at(&switched.prefs, "pack-x", &game, &[])
            .unwrap()
            .unwrap();
        assert!(live.file_by_path(&rel).is_none());
        let stash = game.join("arnold-installer/packs/pack-x");
        let saved = load_state(&stash).unwrap().unwrap();
        assert!(saved.file_by_path(&rel).is_some());
        assert_eq!(fs::read(stash.join(&rel)).unwrap(), v1);
        let mut manifest_v2 = manifest.clone();
        let mut fetcher_v2 = fetcher.clone();
        let url = retarget(&mut manifest_v2, &rel, b"blob-v2");
        fetcher_v2.insert(url, b"blob-v2".to_vec());
        let plan = plan_at(
            &manifest_v2,
            &basic_selection(),
            &switched.prefs,
            "pack-x",
            &game,
            &[],
            true,
        )
        .unwrap();
        assert!(plan.add.iter().any(|file| dest_rel(file) == rel));
        assert!(plan.update.iter().all(|file| dest_rel(file) != rel));
        let err = install_at(
            &plan,
            &switched.prefs,
            "pack-x",
            &ctx(&game),
            &manifest_v2,
            no_loader,
            run_with(&plan, &manifest_v2, &fetcher_v2, false),
        )
        .unwrap_err();
        assert_eq!(err.code(), "P-COLLISION");
        assert_eq!(fs::read(game.join(&rel)).unwrap(), b"players-own-file");
        assert_eq!(kept_aside_paths(&game).unwrap(), vec![rel.clone()]);
        install_at(
            &plan,
            &switched.prefs,
            "pack-x",
            &ctx(&game),
            &manifest_v2,
            no_loader,
            run_with(&plan, &manifest_v2, &fetcher_v2, true),
        )
        .unwrap();
        assert_eq!(fs::read(game.join(&rel)).unwrap(), b"blob-v2");
        assert!(kept_aside_paths(&game).unwrap().is_empty());
    }

    #[test]
    fn occupied_folder_import_is_the_only_way_in_for_ba_plus() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join("game");
        prepare_game(&game);
        fs::write(
            game.join("mods/arnold-snowfall-1.3.2.jar"),
            b"arbitrary-user-bytes",
        )
        .unwrap();
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        let pack_x = Prefs {
            selected_pack: Some("pack-x".into()),
            ..Prefs::default()
        };
        let before = tree_fingerprint(&game);
        let blocked = import_dir_at(&pack_x, "pack-x", &game, &[]).unwrap_err();
        assert_eq!(blocked.code(), "E-FOLDER-OCCUPIED");
        assert_eq!(tree_fingerprint(&game), before);
        let prefs = Prefs {
            selected_pack: Some(BA_PLUS_PACK_ID.into()),
            ..Prefs::default()
        };
        assert_eq!(
            import_dir_at(&prefs, BA_PLUS_PACK_ID, &game, &[]).unwrap(),
            game
        );
        let adopted = adopt_import(&game, &manifest, true).unwrap();
        assert!(adopted.pack_id.is_none());
        assert!(adopted
            .file_by_path("mods/arnold-snowfall-1.3.2.jar")
            .is_some());
        let outcome = apply_change(
            &prefs,
            BA_PLUS_PACK_ID,
            &ctx(&game),
            Change::ChooseFolder(FolderChoice::Keep),
            no_loader,
        )
        .unwrap();
        let entry = outcome.entry.as_ref().unwrap();
        assert!(entry.adopted);
        assert_eq!(
            entry.owner_before,
            Some(FolderOwner::Inferred {
                pack_id: BA_PLUS_PACK_ID.into(),
                source: OwnerSource::LegacyBaPlus,
            })
        );
        assert_eq!(read_active_pack(&game).as_deref(), Some(BA_PLUS_PACK_ID));
        let plan = plan_at(
            &manifest,
            &basic_selection(),
            &outcome.prefs,
            BA_PLUS_PACK_ID,
            &game,
            &[],
            true,
        )
        .unwrap();
        install_at(
            &plan,
            &outcome.prefs,
            BA_PLUS_PACK_ID,
            &ctx(&game),
            &manifest,
            no_loader,
            run_with(&plan, &manifest, &fetcher, false),
        )
        .unwrap();
        assert!(!game.join("mods/arnold-snowfall-1.3.3.jar").exists());
        assert!(!game.join("mods/arnold-snowfall-1.3.2.jar").exists());
        let snaps = list_snapshots(&game).unwrap();
        let newest = snaps.last().unwrap();
        assert_eq!(
            fs::read(
                snapshots_dir(&game)
                    .join(&newest.id)
                    .join("mods/arnold-snowfall-1.3.2.jar")
            )
            .unwrap(),
            b"arbitrary-user-bytes"
        );
        let again = import_dir_at(&outcome.prefs, BA_PLUS_PACK_ID, &game, &[]).unwrap_err();
        assert_eq!(again.code(), "E-STATE");
    }

    #[test]
    fn kept_aside_legacy_file_survives_a_new_filename_without_consent() {
        let tmp = tempdir().unwrap();
        let scratch = tmp.path().join("scratch");
        let game = tmp.path().join("game");
        prepare_game(&game);
        let (manifest, fetcher, _) = test_manifest_and_fetcher();
        stash_real_install(&scratch, &game, "pack-x", &manifest, &fetcher);
        let stash = game.join("arnold-installer/packs/pack-x");
        fs::rename(
            stash.join("mods/arnold-ships-client-0.10.3.jar"),
            stash.join("mods/arnold-ships-client-0.10.2.jar"),
        )
        .unwrap();
        let mut saved = load_state(&stash).unwrap().unwrap();
        for file in &mut saved.files {
            if file.path == "mods/arnold-ships-client-0.10.3.jar" {
                file.path = "mods/arnold-ships-client-0.10.2.jar".into();
            }
        }
        save_state(&stash, &saved).unwrap();
        mark_pack_a(&game, b"pack-a-only");
        fs::write(
            game.join("mods/arnold-ships-client-0.10.2.jar"),
            b"players-own-ships",
        )
        .unwrap();
        let switched = apply_change(
            &shared_prefs("pack-a"),
            "pack-a",
            &ctx(&game),
            Change::SelectPack("pack-x".into()),
            loader(&manifest),
        )
        .unwrap();
        let report = switched.entry.as_ref().unwrap().switch.as_ref().unwrap();
        let rel = "mods/arnold-ships-client-0.10.2.jar";
        assert_eq!(report.conflicts, vec![rel.to_string()]);
        assert_eq!(kept_aside_paths(&game).unwrap(), vec![rel.to_string()]);
        let live = incoming_state_at(&switched.prefs, "pack-x", &game, &[])
            .unwrap()
            .unwrap();
        assert!(live.file_by_path(rel).is_none());
        let plan = plan_at(
            &manifest,
            &basic_selection(),
            &switched.prefs,
            "pack-x",
            &game,
            &[],
            true,
        )
        .unwrap();
        assert!(plan
            .add
            .iter()
            .any(|file| dest_rel(file) == "mods/arnold-ships-client-0.10.3.jar"));
        let before = tree_fingerprint(&game);
        let live_before = load_state(&game).unwrap();
        let err = install_at(
            &plan,
            &switched.prefs,
            "pack-x",
            &ctx(&game),
            &manifest,
            no_loader,
            run_with(&plan, &manifest, &fetcher, false),
        )
        .unwrap_err();
        let payload = err.to_payload();
        assert_eq!(payload.code, "P-COLLISION");
        assert!(payload.detail.contains("kept aside"), "{}", payload.detail);
        assert_eq!(fs::read(game.join(rel)).unwrap(), b"players-own-ships");
        assert!(!game.join("mods/arnold-ships-client-0.10.3.jar").exists());
        assert_eq!(tree_fingerprint(&game), before);
        assert_eq!(load_state(&game).unwrap(), live_before);
        install_at(
            &plan,
            &switched.prefs,
            "pack-x",
            &ctx(&game),
            &manifest,
            no_loader,
            run_with(&plan, &manifest, &fetcher, true),
        )
        .unwrap();
        assert_eq!(
            fs::read(game.join("mods/arnold-ships-client-0.10.3.jar")).unwrap(),
            b"blob-arnold-ships-client-0.10.3.jar"
        );
        assert!(!game.join(rel).exists());
        let snaps = list_snapshots(&game).unwrap();
        let newest = snaps.last().unwrap();
        assert_eq!(
            fs::read(snapshots_dir(&game).join(&newest.id).join(rel)).unwrap(),
            b"players-own-ships"
        );
        assert!(kept_aside_paths(&game).unwrap().is_empty());
    }

    #[test]
    fn plan_from_outgoing_state_is_rejected_after_transition() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join("game");
        prepare_game(&game);
        legacy_shared_game(&game);
        let outgoing = load_state(&game).unwrap().unwrap();
        let (manifest, _, _) = test_manifest_and_fetcher();
        let mut plan = build_plan(&manifest, &basic_selection(), Some(&outgoing), true).unwrap();
        plan.pack_id = "pack-x".into();
        plan.pack_dir = game.to_string_lossy().into_owned();
        assert!(
            !plan.update.is_empty() || !plan.remove.is_empty(),
            "fixture must differ from an empty install"
        );
        let prefs = Prefs {
            selected_pack: Some("pack-x".into()),
            ..Prefs::default()
        };
        apply_change(
            &prefs,
            "pack-x",
            &ctx(&game),
            Change::UseDefaultFolder(true),
            loader(&manifest),
        )
        .unwrap();
        let err = ensure_plan_current(
            &plan,
            "pack-x",
            &game,
            &manifest,
            load_state(&game).unwrap().as_ref(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not match"), "{err}");
    }

    #[test]
    fn disabling_default_folder_moves_nothing() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join("game");
        prepare_game(&game);
        legacy_shared_game(&game);
        let before = tree_fingerprint(&game);
        let prefs = Prefs {
            selected_pack: Some("pack-x".into()),
            ..Prefs::default()
        };
        let disabled = apply_change(
            &prefs,
            "pack-x",
            &ctx(&game),
            Change::UseDefaultFolder(false),
            no_loader,
        )
        .unwrap();
        assert!(disabled.entry.is_none());
        assert_eq!(tree_fingerprint(&game), before);
        let fresh = apply_change(
            &disabled.prefs,
            "pack-x",
            &ctx(&game),
            Change::ChooseFolder(FolderChoice::Fresh),
            no_loader,
        )
        .unwrap();
        assert!(fresh.entry.is_none());
        assert_eq!(
            fresh.prefs.pack_locations.get("pack-x"),
            Some(&PackLocation::OwnFolder)
        );
        assert_eq!(tree_fingerprint(&game), before);
    }

    #[test]
    fn switch_shared_requires_shared_mode() {
        let tmp = tempdir().unwrap();
        let game = tmp.path().join("game");
        prepare_game(&game);
        legacy_shared_game(&game);
        let before = tree_fingerprint(&game);
        let err = apply_change(
            &Prefs::default(),
            "pack-a",
            &ctx(&game),
            Change::SwitchShared("pack-x".into()),
            no_loader,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("pack switching only applies"),
            "{err}"
        );
        assert_eq!(tree_fingerprint(&game), before);
    }

    #[test]
    fn destination_commands_route_through_orchestration() {
        let source = include_str!("commands.rs");
        let checks = [
            ("pub fn select_pack(", "Change::SelectPack"),
            ("pub fn set_game_dir(", "Change::SetGameDir"),
            ("pub fn set_use_default_folder(", "Change::UseDefaultFolder"),
            ("pub fn choose_pack_folder(", "Change::ChooseFolder"),
            ("pub fn switch_shared_pack(", "Change::SwitchShared"),
            ("pub async fn install(", "destination::install_at"),
            ("pub async fn repair(", "destination::prepare_destination"),
            ("pub fn confirm_import(", "destination::import_dir_at"),
        ];
        for (signature, needle) in checks {
            let start = source
                .find(signature)
                .unwrap_or_else(|| panic!("missing {signature}"));
            let rest = &source[start..];
            let end = rest
                .find("\n}\n")
                .unwrap_or_else(|| panic!("unclosed {signature}"));
            assert!(
                rest[..end].contains(needle),
                "{signature} does not call {needle}"
            );
        }
        let mut found = 0;
        let mut offset = 0;
        while let Some(rel) = source[offset..].find("save_prefs(") {
            let at = offset + rel;
            let before = &source[..at];
            let fn_at = before.rfind("fn ").expect("fn before save_prefs");
            let line_start = before[..fn_at]
                .rfind('\n')
                .map(|index| index + 1)
                .unwrap_or(0);
            let line_end = source[line_start..]
                .find('\n')
                .map(|index| line_start + index)
                .unwrap_or(source.len());
            let line = &source[line_start..line_end];
            assert!(
                line.contains("fn run_destination_change(")
                    || line.contains("fn set_tour_done(")
                    || line.contains("fn set_launcher_prefs("),
                "{line}"
            );
            found += 1;
            offset = at + "save_prefs(".len();
        }
        assert!(found >= 3, "save_prefs call sites disappeared");
    }
}
