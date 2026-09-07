//! Saving and reloading a drawing session.
//!
//! A session is the shape list plus enough context to put the canvas back the
//! way it was: which slate was showing and what the step-badge counter was up
//! to. It is plain JSON so it stays diffable and hand-editable, and it is
//! versioned so a future format change can be detected rather than guessed at.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
    IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{PCWSTR, w};

use crate::config::AppConfig;
use crate::types::{Annotation, CanvasBackground, Shape, ShapeId};

/// Bumped only when the on-disk shape changes incompatibly.
pub const SESSION_VERSION: u32 = 2;

const FILE_PREFIX: &str = "Zoomify_Session_";
const FILE_SUFFIX: &str = ".json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    /// Local wall-clock stamp, for display; the filename is what sorts.
    pub saved_at: String,
    pub background: String,
    pub step_counter: u32,
    /// v1 files stored bare shapes with no identity. They still load: each one
    /// is handed a fresh id on the way in.
    #[serde(deserialize_with = "de_annotations")]
    pub shapes: Vec<Annotation>,
}

fn de_annotations<'de, D>(deserializer: D) -> Result<Vec<Annotation>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        Modern(Vec<Annotation>),
        Legacy(Vec<Shape>),
    }
    Ok(match Either::deserialize(deserializer)? {
        Either::Modern(v) => v,
        Either::Legacy(v) => v.into_iter().map(Annotation::new).collect(),
    })
}

impl Session {
    pub fn new(shapes: Vec<Annotation>, background: CanvasBackground, step_counter: u32) -> Self {
        Self {
            version: SESSION_VERSION,
            saved_at: local_stamp_readable(),
            background: background_name(background).to_string(),
            step_counter,
            shapes,
        }
    }

    pub fn background_enum(&self) -> CanvasBackground {
        match self.background.as_str() {
            "Whiteboard" => CanvasBackground::Whiteboard,
            "Blackboard" => CanvasBackground::Blackboard,
            _ => CanvasBackground::Transparent,
        }
    }
}

pub fn background_name(bg: CanvasBackground) -> &'static str {
    match bg {
        CanvasBackground::Transparent => "Transparent",
        CanvasBackground::Whiteboard => "Whiteboard",
        CanvasBackground::Blackboard => "Blackboard",
    }
}

/// The user's Pictures folder, honouring OneDrive/Known Folder redirection.
pub fn pictures_dir() -> PathBuf {
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

    unsafe {
        if let Ok(pwstr) = SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None)
            && !pwstr.is_null()
        {
            let path = pwstr.to_string().ok().map(PathBuf::from);
            CoTaskMemFree(Some(pwstr.0 as *const std::ffi::c_void));
            if let Some(p) = path {
                return p;
            }
        }
    }

    // Fall back only if the shell call fails outright.
    std::env::var_os("USERPROFILE")
        .map(|u| PathBuf::from(u).join("Pictures"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Where sessions live: the configured folder, else Pictures\Zoomify Sessions.
pub fn sessions_dir(cfg: &AppConfig) -> PathBuf {
    let configured = cfg.session_folder.trim();
    if configured.is_empty() {
        pictures_dir().join("Zoomify Sessions")
    } else {
        PathBuf::from(configured)
    }
}

/// Sortable local timestamp used in filenames.
fn local_stamp() -> String {
    unsafe {
        let t = windows::Win32::System::SystemInformation::GetLocalTime();
        format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        )
    }
}

fn local_stamp_readable() -> String {
    unsafe {
        let t = windows::Win32::System::SystemInformation::GetLocalTime();
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        )
    }
}

/// A free path for a new session file. Second-resolution stamps collide when
/// saving twice quickly, so a numeric suffix is added rather than overwriting.
pub fn next_session_path(dir: &Path) -> PathBuf {
    let stamp = local_stamp();
    let mut path = dir.join(format!("{}{}{}", FILE_PREFIX, stamp, FILE_SUFFIX));
    let mut n = 2;
    while path.exists() && n < 1000 {
        path = dir.join(format!("{}{}_{}{}", FILE_PREFIX, stamp, n, FILE_SUFFIX));
        n += 1;
    }
    path
}

pub fn save(path: &Path, session: &Session) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create folder: {}", e))?;
    }
    let json = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn load(path: &Path) -> Result<Session, String> {
    let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    // Editors love to add a BOM; serde_json chokes on it.
    let data = data.strip_prefix('\u{feff}').unwrap_or(&data);
    let session: Session = serde_json::from_str(data).map_err(|e| e.to_string())?;
    if session.version > SESSION_VERSION {
        return Err(format!(
            "saved by a newer Zoomify (format v{}, this build reads v{})",
            session.version, SESSION_VERSION
        ));
    }
    // Newly drawn annotations must not collide with ids that came off disk.
    if let Some(highest) = session.shapes.iter().map(|a| a.id).max() {
        ShapeId::reserve_above(highest);
    }
    Ok(session)
}

/// Session files in `dir`, newest first. Names embed a sortable timestamp, so
/// a reverse lexicographic sort is a chronological one.
pub fn list_sessions(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_session_file(p))
        .collect();
    found.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    found
}

pub fn is_session_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with(FILE_PREFIX) && n.ends_with(FILE_SUFFIX))
        .unwrap_or(false)
}

/// Delete all but the newest `keep_last` sessions, along with the PNG each one
/// may have been exported next to. `keep_last == 0` means keep everything.
///
/// Returns how many sessions were removed.
pub fn prune(dir: &Path, keep_last: usize) -> usize {
    if keep_last == 0 {
        return 0;
    }
    let sessions = list_sessions(dir);
    if sessions.len() <= keep_last {
        return 0;
    }
    let mut removed = 0;
    for path in &sessions[keep_last..] {
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
            let _ = std::fs::remove_file(path.with_extension("png"));
        }
    }
    removed
}

// ─────────────────────── Shell pickers ───────────────────────

/// The last session error worth telling the user about, when it happened
/// somewhere no toast can be shown (autosave runs as the overlay tears down).
/// The host drains this and raises a tray balloon.
pub static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

pub fn report_error(msg: String) {
    if let Ok(mut slot) = LAST_ERROR.lock() {
        *slot = Some(msg);
    }
}

pub fn take_error() -> Option<String> {
    LAST_ERROR.lock().ok().and_then(|mut slot| slot.take())
}

fn shell_item_for(dir: &Path) -> Option<IShellItem> {
    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok() }
}

fn run_dialog(owner: HWND, configure: impl FnOnce(&IFileOpenDialog)) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        configure(&dialog);
        // A cancelled dialog returns an error HRESULT; that is not a failure.
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(name.0 as *const std::ffi::c_void));
        path
    }
}

/// Ask for an existing session file. `start_in` seeds the initial folder.
pub fn pick_session_file(owner: HWND, start_in: &Path) -> Option<PathBuf> {
    let start = shell_item_for(start_in);
    run_dialog(owner, |dialog| unsafe {
        let _ = dialog.SetTitle(w!("Open Zoomify Session"));
        let filters = [COMDLG_FILTERSPEC {
            pszName: w!("Zoomify session (*.json)"),
            pszSpec: w!("*.json"),
        }];
        let _ = dialog.SetFileTypes(&filters);
        let _ = dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST);
        if let Some(item) = &start {
            let _ = dialog.SetFolder(item);
        }
    })
}

/// Ask for a folder to store sessions in.
pub fn pick_folder(owner: HWND, start_in: &Path) -> Option<PathBuf> {
    let start = shell_item_for(start_in);
    run_dialog(owner, |dialog| unsafe {
        let _ = dialog.SetTitle(w!("Choose Session Folder"));
        let _ = dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_PICKFOLDERS | FOS_PATHMUSTEXIST);
        if let Some(item) = &start {
            let _ = dialog.SetFolder(item);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColorPreset, Point2D, StrokePattern};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zoomify_session_test_{}", tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_shapes() -> Vec<Annotation> {
        [
            Shape::Stroke {
                points: vec![Point2D::new(1.0, 2.0), Point2D::new(3.0, 4.5)],
                color: ColorPreset::Custom(12, 240, 7),
                width: 4.0,
                is_highlighter: true,
                pressures: Vec::new(),
                pattern: StrokePattern::Dashed,
            },
            Shape::Text {
                origin: Point2D::new(10.0, 20.0),
                text: "line one\nline two".to_string(),
                font_size: 22.0,
                color: ColorPreset::Red,
                is_bold: true,
                is_italic: false,
                card_style: crate::types::TextCardStyle::Badge,
                font_family: crate::types::TextFontFamily::CascadiaCode,
            },
            Shape::Image {
                start: Point2D::new(0.0, 0.0),
                end: Point2D::new(4.0, 4.0),
                pixels: crate::types::ImagePixels {
                    width: 2,
                    height: 2,
                    bgra: (0..16u8).collect(),
                },
            },
        ]
        .into_iter()
        .map(Annotation::new)
        .collect()
    }

    #[test]
    fn test_v1_sessions_still_load_and_get_fresh_ids() {
        // v1 stored bare shapes, with no identity at all.
        let dir = temp_dir("v1");
        let path = dir.join("Zoomify_Session_20240101-000000.json");
        std::fs::write(
            &path,
            r#"{"version":1,"saved_at":"","background":"Whiteboard","step_counter":3,
                "shapes":[{"Text":{"origin":{"x":1.0,"y":2.0},"text":"hi","font_size":20.0,
                "color":"Red","is_bold":false,"is_italic":false,"card_style":"Badge",
                "font_family":"SegoeUI"}}]}"#,
        )
        .unwrap();

        let s = load(&path).unwrap();
        assert_eq!(s.shapes.len(), 1);
        assert_eq!(s.step_counter, 3);
        assert!(s.shapes[0].container.is_none());
        // It got an id, and the counter was pushed past it so nothing collides.
        let assigned = s.shapes[0].id;
        assert!(ShapeId::fresh() > assigned);
    }

    #[test]
    fn test_session_round_trips_every_field() {
        let dir = temp_dir("roundtrip");
        let session = Session::new(sample_shapes(), CanvasBackground::Blackboard, 7);
        let path = next_session_path(&dir);
        save(&path, &session).unwrap();

        let back = load(&path).unwrap();
        assert_eq!(back.version, SESSION_VERSION);
        assert_eq!(back.step_counter, 7);
        assert_eq!(back.background_enum(), CanvasBackground::Blackboard);
        // Custom colours and embedded newlines are the fiddly parts.
        assert_eq!(back.shapes, session.shapes);
    }

    #[test]
    fn test_load_rejects_a_newer_format() {
        let dir = temp_dir("newer");
        let path = dir.join("Zoomify_Session_29990101-000000.json");
        std::fs::write(
            &path,
            r#"{"version":99,"saved_at":"","background":"Transparent","step_counter":1,"shapes":[]}"#,
        )
        .unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.contains("newer"), "{}", err);
    }

    #[test]
    fn test_load_tolerates_a_utf8_bom() {
        let dir = temp_dir("bom");
        let path = dir.join("Zoomify_Session_20240101-000000.json");
        std::fs::write(
            &path,
            "\u{feff}{\"version\":1,\"saved_at\":\"\",\"background\":\"Whiteboard\",\"step_counter\":2,\"shapes\":[]}",
        )
        .unwrap();
        let s = load(&path).unwrap();
        assert_eq!(s.step_counter, 2);
        assert_eq!(s.background_enum(), CanvasBackground::Whiteboard);
    }

    #[test]
    fn test_list_sessions_is_newest_first_and_ignores_strangers() {
        let dir = temp_dir("list");
        for stamp in ["20240101-000000", "20240103-000000", "20240102-000000"] {
            std::fs::write(dir.join(format!("Zoomify_Session_{}.json", stamp)), "{}").unwrap();
        }
        std::fs::write(dir.join("notes.json"), "{}").unwrap();
        std::fs::write(dir.join("Zoomify_Session_20240104-000000.png"), "x").unwrap();

        let found = list_sessions(&dir);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                "Zoomify_Session_20240103-000000.json",
                "Zoomify_Session_20240102-000000.json",
                "Zoomify_Session_20240101-000000.json",
            ]
        );
    }

    #[test]
    fn test_prune_keeps_the_newest_n_and_their_pngs() {
        let dir = temp_dir("prune");
        for stamp in ["20240101-000000", "20240102-000000", "20240103-000000"] {
            std::fs::write(dir.join(format!("Zoomify_Session_{}.json", stamp)), "{}").unwrap();
            std::fs::write(dir.join(format!("Zoomify_Session_{}.png", stamp)), "x").unwrap();
        }
        assert_eq!(prune(&dir, 2), 1);

        let left = list_sessions(&dir);
        assert_eq!(left.len(), 2);
        // The oldest session took its exported PNG with it.
        assert!(!dir.join("Zoomify_Session_20240101-000000.png").exists());
        assert!(dir.join("Zoomify_Session_20240103-000000.png").exists());
    }

    #[test]
    fn test_prune_zero_keeps_everything() {
        let dir = temp_dir("prune_zero");
        for stamp in ["20240101-000000", "20240102-000000"] {
            std::fs::write(dir.join(format!("Zoomify_Session_{}.json", stamp)), "{}").unwrap();
        }
        assert_eq!(prune(&dir, 0), 0);
        assert_eq!(list_sessions(&dir).len(), 2);
    }

    #[test]
    fn test_next_session_path_does_not_overwrite() {
        let dir = temp_dir("collide");
        let a = next_session_path(&dir);
        std::fs::write(&a, "{}").unwrap();
        let b = next_session_path(&dir);
        assert_ne!(a, b);
    }
}
