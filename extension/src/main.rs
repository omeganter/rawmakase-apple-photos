//! Apple Photos Import: a RAWmakase extension (macOS).
//!
//! The command starts a helper app that copies the Photos library to a
//! folder with PhotoKit; this program then reports the copied files through
//! its photo source, so RAWmakase adds them to the open catalog, and makes
//! the albums as collections over the control socket.
//!
//! The helper is a separate app bundle started through Launch Services
//! because macOS attributes a Photos request to the app that started the
//! process: run as this program's child it is denied without a prompt, since
//! RAWmakase's Info.plist has no NSPhotoLibraryUsageDescription.
mod control;
mod plan;

use rawmakase_extension::protocol::method::{CommandRun, Level, SourceReport, SourceStarted};
use rawmakase_extension::{Context, Extension, Host, RemoteError, Source};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The helper app, in the package beside the program's folder.
const HELPER: &str = "helper/FotosARawmakase.app";
/// Files per `source.report` (the protocol takes up to 200).
const REPORT: usize = 100;
/// Photo ids per control request (a request is at most 64 KiB).
const IDS: usize = 400;
/// Stacks per control request.
const STACKS: usize = 500;

#[derive(Default)]
struct State {
    /// The source's running scope: the open catalog it is on for.
    scope: Mutex<Option<u64>>,
    /// An import is running.
    running: AtomicBool,
    /// The folder of the import running, to ask its helper to stop.
    dest: Mutex<Option<PathBuf>>,
}

struct Photos(Arc<State>);

impl Source for Photos {
    fn start(&mut self, _host: Host, started: SourceStarted) -> Result<(), RemoteError> {
        eprintln!("source started: scope {} for {}", started.scope, started.catalog);
        *self.0.scope.lock().unwrap() = Some(started.scope);
        Ok(())
    }
    fn stop(&mut self, scope: u64) {
        let mut current = self.0.scope.lock().unwrap();
        if *current == Some(scope) {
            *current = None;
        }
    }
}

/// Starts the import on a thread and answers at once: a command has 30
/// seconds, a library takes hours.
fn import(state: &Arc<State>, cx: &Context) -> Result<Value, RemoteError> {
    let host = cx.host().clone();
    let settings = cx.settings().clone();
    let dest = settings.get("destination").and_then(Value::as_str).unwrap_or_default();
    eprintln!("import: destination {dest:?}, scope {:?}", state.scope.lock().unwrap());
    if dest.is_empty() {
        host.notify(Level::Warning, "Apple Photos: choose the folder to copy photos to, in Preferences > Extensions > Apple Photos Import.")?;
        return Ok(json!({"started": false}));
    }
    if state.scope.lock().unwrap().is_none() {
        host.notify(Level::Warning, "Apple Photos: turn on \"Apple Photos\" under Photo sources for this catalog, in Preferences > Extensions > Apple Photos Import.")?;
        return Ok(json!({"started": false}));
    }
    if state.running.swap(true, Ordering::SeqCst) {
        host.notify(Level::Info, "Apple Photos: an import is already running.")?;
        return Ok(json!({"started": false}));
    }
    let dest = PathBuf::from(dest);
    *state.dest.lock().unwrap() = Some(dest.clone());
    let state = state.clone();
    std::thread::spawn(move || {
        let outcome = run(&host, &state, &dest, &settings);
        eprintln!("import finished: {outcome:?}");
        let _ = host.status(None);
        let _ = match outcome {
            Ok(text) => host.notify(Level::Info, &text),
            Err(text) => host.notify(Level::Error, &format!("Apple Photos: {text}")),
        };
        state.running.store(false, Ordering::SeqCst);
    });
    Ok(json!({"started": true}))
}

/// Asks the helper to stop after the photo it is copying. What it copied
/// stays, and the next import goes on from there.
fn stop(state: &Arc<State>, cx: &Context) -> Result<Value, RemoteError> {
    let dest = state.dest.lock().unwrap().clone();
    match dest {
        Some(dest) if state.running.load(Ordering::SeqCst) => {
            let _ = std::fs::write(dest.join("_fotos/cancelar"), "");
            cx.host().notify(Level::Info, "Apple Photos: stopping after the current photo.")?;
            Ok(json!({"stopping": true}))
        }
        _ => Ok(json!({"stopping": false})),
    }
}

fn run(host: &Host, state: &State, dest: &Path, settings: &Map<String, Value>) -> Result<String, String> {
    copy(host, dest, settings)?;
    let text = std::fs::read_to_string(dest.join("_fotos/manifiesto.json"))
        .map_err(|e| format!("the export left no manifest: {e}"))?;
    let manifest = serde_json::from_str(&text).map_err(|e| format!("the export's manifest can't be read: {e}"))?;
    let plan = plan::read(&manifest, dest)?;

    // Add the files to the catalog, and learn their photos' ids.
    let mut ids: HashMap<PathBuf, i64> = HashMap::new();
    let (mut added, mut refused) = (0u32, 0usize);
    for (n, files) in plan.files.chunks(REPORT).enumerate() {
        let _ = host.status(Some(&format!("Apple Photos: adding {} of {}", n * REPORT, plan.files.len())));
        let scope = state.scope.lock().unwrap().ok_or("the catalog was closed, or the photo source turned off, before the photos were added. Run the import again: nothing is copied twice.")?;
        let reported = host
            .report(SourceReport {
                source: "photos".into(),
                scope,
                folder: dest.to_path_buf(),
                files: files.to_vec(),
                show: false,
            })
            .map_err(|e| format!("RAWmakase didn't add the photos: {e:?}"))?;
        added += reported.added;
        refused += reported.refused.len();
        ids.extend(reported.photos.into_iter().map(|p| (p.path, p.photo)));
    }

    let made = collections(host, &plan, &ids)?;
    let mut text = format!("Apple Photos: {} photos in the catalog ({added} new)", ids.len());
    match made {
        Some((albums, Some(stacks))) => text += &format!(", {albums} albums, {stacks} stacks"),
        Some((albums, None)) => text += &format!(", {albums} albums. Edited versions are beside their originals, not stacked: this RAWmakase has no stack command"),
        None => text += ". Albums were not made: turn on external control in Preferences, then run the import again",
    }
    if refused > 0 {
        text += &format!(". {refused} files were not added");
    }
    if !plan.verified {
        text += &format!(". Reading keywords and people is not verified on macOS {}: check a few photos", plan.macos);
    }
    Ok(text + ".")
}

/// Runs the helper and waits for it, showing its progress.
fn copy(host: &Host, dest: &Path, settings: &Map<String, Value>) -> Result<(), String> {
    let helper = std::env::current_dir().map_err(|e| e.to_string())?.join(HELPER);
    if !helper.exists() {
        return Err(format!("the helper app is missing from the extension ({})", helper.display()));
    }
    // RAWmakase unpacks a package's included files without their executable
    // bit; only the program gets it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let binary = helper.join("Contents/MacOS/fotos-a-rawmakase");
        let _ = std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755));
    }
    let log = dest.join("_fotos/exportacion.log");
    let _ = std::fs::remove_file(dest.join("_fotos/cancelar"));
    let _ = std::fs::remove_file(&log);
    let mut open = Command::new("/usr/bin/open");
    open.arg("-n").arg("-W").arg(&helper).arg("--args").arg("--dest").arg(dest);
    for (setting, flag) in [("videos", "--videos"), ("bursts", "--rafagas"), ("hidden", "--ocultas")] {
        if settings.get(setting).and_then(Value::as_bool) == Some(true) {
            open.arg(flag);
        }
    }
    if let Some(album) = settings.get("album").and_then(Value::as_str).filter(|a| !a.is_empty()) {
        open.arg("--album").arg(album);
    }
    let mut child = open.spawn().map_err(|e| format!("the helper app didn't start: {e}"))?;
    let _ = host.status(Some("Apple Photos: reading the library"));
    let last = loop {
        let last = std::fs::read_to_string(&log)
            .ok()
            .and_then(|t| t.lines().last().map(str::to_string))
            .unwrap_or_default();
        if let Some((done, total)) = plan::progress(&last) {
            let _ = host.status(Some(&format!("Apple Photos: copying {done} of {total}")));
        }
        match child.try_wait() {
            Ok(None) => std::thread::sleep(Duration::from_secs(1)),
            _ => break last,
        }
    };
    if last.starts_with("FIN") {
        Ok(())
    } else if last.starts_with("CANCELADO") {
        Err("stopped. What was copied stays; run the import again to go on.".into())
    } else if last.starts_with("Sin permiso") {
        Err("no access to Photos. Allow it for FotosARawmakase in System Settings > Privacy & Security > Photos.".into())
    } else {
        Err(format!("the copy didn't finish: {last}"))
    }
}

/// Makes the albums and their folders, with the photos' ids, then the
/// stacks where this RAWmakase has the `stack` command. `None` when external
/// control is off; the stacks' count is `None` without the command. An album
/// or folder already there is used, not made twice.
fn collections(
    host: &Host,
    plan: &plan::Plan,
    ids: &HashMap<PathBuf, i64>,
) -> Result<Option<(usize, Option<usize>)>, String> {
    // The extension's data folder is <data>/extension-data/<id>.
    let data = host.data_dir();
    let Some(mut control) = data.parent().and_then(Path::parent).and_then(control::Control::open) else {
        return Ok(None);
    };
    let mut existing: HashMap<(Option<i64>, String), i64> = HashMap::new();
    for c in control.request("collections", json!({}))?["collections"].as_array().into_iter().flatten() {
        if let (Some(id), Some(name)) = (c["id"].as_i64(), c["name"].as_str()) {
            existing.insert((c["parent_id"].as_i64(), name.to_string()), id);
        }
    }
    let mut made: HashMap<&str, i64> = HashMap::new();
    let mut albums = 0;
    for (n, c) in plan.collections.iter().enumerate() {
        let _ = host.status(Some(&format!("Apple Photos: album {} of {}", n + 1, plan.collections.len())));
        let parent = c.parent.as_deref().and_then(|p| made.get(p).copied());
        let photos: Vec<i64> = c.files.iter().filter_map(|f| ids.get(f).copied()).collect();
        let mut rest = &photos[..];
        let id = match existing.get(&(parent, c.name.clone())) {
            Some(id) => *id,
            None => {
                let first = &rest[..rest.len().min(IDS)];
                rest = &rest[first.len()..];
                let mut fields = json!({"name": c.name, "kind": if c.set { "set" } else { "collection" }});
                if let Some(parent) = parent {
                    fields["parent_id"] = json!(parent);
                }
                if !c.set {
                    fields["photo_ids"] = json!(first);
                }
                control.request("create_collection", fields)?["created"]["id"]
                    .as_i64()
                    .ok_or("create_collection answered without an id")?
            }
        };
        made.insert(&c.id, id);
        if c.set {
            continue;
        }
        for chunk in rest.chunks(IDS) {
            control.request("add_to_collection", json!({"collection_id": id, "photo_ids": chunk}))?;
        }
        albums += 1;
    }
    Ok(Some((albums, stacks(host, &mut control, plan, ids)?)))
}

/// Stacks each edited version over its original, and each burst. Stacking
/// again what is stacked already makes the same stacks.
fn stacks(
    host: &Host,
    control: &mut control::Control,
    plan: &plan::Plan,
    ids: &HashMap<PathBuf, i64>,
) -> Result<Option<usize>, String> {
    let commands = control.request("capabilities", json!({}))?;
    if !commands["commands"].as_array().is_some_and(|c| c.iter().any(|c| c == "stack")) {
        return Ok(None);
    }
    let all: Vec<Vec<i64>> = plan
        .stacks
        .iter()
        .map(|s| s.iter().filter_map(|f| ids.get(f).copied()).collect::<Vec<i64>>())
        .filter(|s| s.len() > 1)
        .collect();
    for (n, chunk) in all.chunks(STACKS).enumerate() {
        let _ = host.status(Some(&format!("Apple Photos: stacking {} of {}", n * STACKS, all.len())));
        control.request("stack", json!({"stacks": chunk}))?;
    }
    Ok(Some(all.len()))
}

fn extension() -> Extension {
    let state = Arc::new(State::default());
    let (for_import, for_stop) = (state.clone(), state.clone());
    Extension::new()
        .source("photos", Photos(state))
        .command("import", move |cx: &Context, _: CommandRun| import(&for_import, cx))
        .command("stop", move |cx: &Context, _: CommandRun| stop(&for_stop, cx))
}

fn main() -> std::io::Result<()> {
    extension().run()
}

#[cfg(test)]
mod tests {
    use rawmakase_extension::mock::MockHost;
    use serde_json::json;

    #[test]
    fn import_without_a_destination_asks_for_one() {
        let host = MockHost::start_with(super::extension(), |_, _| Ok(json!(null)));
        host.hello().unwrap();
        let reply = host.run_command("import", None, json!(null)).unwrap();
        assert_eq!(reply, json!({"started": false}));
        host.finish().unwrap();
    }
}
