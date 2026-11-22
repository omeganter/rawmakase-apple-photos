//! What the helper's manifest (`_fotos/manifiesto.json`) says to do: which
//! files to report, and which collections to make of them. No I/O here.
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The newest manifest version this program reads.
pub const MANIFEST_MAX: u64 = 2;

/// The keys of a photo's entry that name a file RAWmakase reads as a photo.
const PHOTO_FILES: [&str; 3] = ["original", "raw", "editada"];

/// An album or a folder of albums, in the manifest's order (a folder before
/// what it holds).
#[derive(Debug, PartialEq)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    /// A folder of albums: a collection set.
    pub set: bool,
    /// Its photos' files, in the album's order.
    pub files: Vec<PathBuf>,
}

#[derive(Debug, PartialEq)]
pub struct Plan {
    pub files: Vec<PathBuf>,
    pub collections: Vec<Collection>,
    /// What belongs together, each list a stack with its top first: a
    /// photo's edited version over its original and its RAW, and a burst's
    /// photos in the order Photos has them.
    pub stacks: Vec<Vec<PathBuf>>,
    /// macOS version the library was read on, and whether reading its
    /// database was verified for that version.
    pub macos: String,
    pub verified: bool,
}

pub fn read(manifest: &Value, dest: &Path) -> Result<Plan, String> {
    let version = manifest["version"].as_u64().unwrap_or(1);
    if version > MANIFEST_MAX {
        return Err(format!(
            "the export's manifest is version {version}; this extension reads up to {MANIFEST_MAX}. Update the extension."
        ));
    }
    let mut files = Vec::new();
    let mut by_photo = std::collections::HashMap::new();
    let mut stacks: Vec<Vec<PathBuf>> = Vec::new();
    let mut bursts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for photo in manifest["fotos"].as_array().into_iter().flatten() {
        if photo["tipo"].as_str() != Some("foto") {
            continue;
        }
        let mine: Vec<PathBuf> = PHOTO_FILES
            .iter()
            .filter_map(|k| photo[*k].as_str())
            .map(|rel| dest.join(rel))
            .collect();
        files.extend(mine.iter().cloned());
        // The edited version on top, as Photos shows the photo.
        let top_first: Vec<PathBuf> = ["editada", "original", "raw"]
            .iter()
            .filter_map(|k| photo[*k].as_str())
            .map(|rel| dest.join(rel))
            .collect();
        match photo["rafaga"].as_str() {
            // A burst is one stack; stacks hold the photos of one folder, so
            // a burst across two months' folders is one stack in each.
            Some(burst) => {
                let folder = top_first[0].parent().map(Path::to_path_buf).unwrap_or_default();
                let key = format!("{burst}\n{}", folder.display());
                let at = *bursts.entry(key).or_insert_with(|| {
                    stacks.push(Vec::new());
                    stacks.len() - 1
                });
                stacks[at].extend(top_first);
            }
            None if top_first.len() > 1 => stacks.push(top_first),
            None => {}
        }
        if let Some(id) = photo["id"].as_str() {
            by_photo.insert(id.to_string(), mine);
        }
    }
    let collections = manifest["colecciones"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(Collection {
                id: c["id"].as_str()?.to_string(),
                name: c["nombre"].as_str()?.to_string(),
                parent: c["padre"].as_str().map(str::to_string),
                set: c["tipo"].as_str() == Some("carpeta"),
                files: c["fotos"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|id| by_photo.get(id.as_str()?))
                    .flatten()
                    .cloned()
                    .collect(),
            })
        })
        .collect();
    stacks.retain(|s| s.len() > 1);
    Ok(Plan {
        files,
        collections,
        stacks,
        macos: manifest["origen"]["macos"].as_str().unwrap_or("unknown").to_string(),
        verified: manifest["origen"]["macos_comprobado"].as_bool().unwrap_or(false),
    })
}

/// "123/456 · …" at the start of the helper's last progress line.
pub fn progress(line: &str) -> Option<(u64, u64)> {
    let head = line.split(' ').next()?;
    let (done, total) = head.split_once('/')?;
    Some((done.parse().ok()?, total.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_photos_and_albums_in_order() {
        let m = json!({
            "version": 2,
            "origen": {"macos": "27.0.1", "macos_comprobado": true},
            "fotos": [
                {"id": "A", "tipo": "foto", "original": "2019/2019-10/a.heic", "editada": "2019/2019-10/a_editada.jpeg"},
                {"id": "B", "tipo": "video", "original": "2019/2019-10/b.mov"},
                {"id": "C", "tipo": "foto", "original": "2019/2019-10/c.jpg", "video_live": "2019/2019-10/c.mov"}
            ],
            "colecciones": [
                {"id": "F", "nombre": "Trips", "tipo": "carpeta", "padre": null},
                {"id": "L", "nombre": "Boston", "tipo": "album", "padre": "F", "fotos": ["C", "B", "A"]}
            ]
        });
        let plan = read(&m, Path::new("/d")).unwrap();
        assert_eq!(plan.files.len(), 3, "a video and a Live Photo's video are not reported");
        assert!(plan.verified);
        assert!(plan.collections[0].set && plan.collections[0].files.is_empty());
        assert_eq!(
            plan.stacks,
            [["/d/2019/2019-10/a_editada.jpeg", "/d/2019/2019-10/a.heic"].map(PathBuf::from)],
            "the edited version is the top; a photo alone is no stack"
        );
        let album = &plan.collections[1];
        assert_eq!(album.parent.as_deref(), Some("F"));
        assert_eq!(
            album.files,
            ["/d/2019/2019-10/c.jpg", "/d/2019/2019-10/a.heic", "/d/2019/2019-10/a_editada.jpeg"]
                .map(PathBuf::from)
        );
    }

    #[test]
    fn a_burst_is_one_stack_per_folder() {
        let m = json!({"version": 2, "fotos": [
            {"id": "A", "tipo": "foto", "original": "2019/2019-10/a.jpg", "rafaga": "B1"},
            {"id": "B", "tipo": "foto", "original": "2019/2019-10/b.jpg", "rafaga": "B1"},
            {"id": "C", "tipo": "foto", "original": "2019/2019-11/c.jpg", "rafaga": "B1"}
        ]});
        let plan = read(&m, Path::new("/d")).unwrap();
        assert_eq!(
            plan.stacks,
            [["/d/2019/2019-10/a.jpg", "/d/2019/2019-10/b.jpg"].map(PathBuf::from)]
        );
    }

    #[test]
    fn refuses_a_newer_manifest() {
        assert!(read(&json!({"version": 3}), Path::new("/d")).is_err());
    }

    #[test]
    fn reads_the_helpers_progress() {
        assert_eq!(progress("300/12785 · copiados 310"), Some((300, 12785)));
        assert_eq!(progress("FIN · elementos 3"), None);
    }
}
