//! Where the app keeps its data (spec §2) and where downloads go by default.

use std::path::PathBuf;

use tauri::{AppHandle, Manager as _, Runtime};

/// The app-data folder name under the OS data dir (spec §2).
pub const DATA_FOLDER: &str = "muzn-dm";

/// Resolved paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    /// `<OS data dir>/muzn-dm`.
    pub data_dir: PathBuf,
    /// `<data_dir>/mdm.db`.
    pub db: PathBuf,
    /// The OS Downloads folder, else the home folder.
    pub download_dir: PathBuf,
}

impl AppPaths {
    /// Build from the OS roots (pure; tested).
    pub fn from_roots(data_root: PathBuf, downloads: Option<PathBuf>, home: PathBuf) -> AppPaths {
        let data_dir = data_root.join(DATA_FOLDER);
        AppPaths {
            db: data_dir.join("mdm.db"),
            data_dir,
            download_dir: downloads.unwrap_or(home),
        }
    }

    /// Ask the OS through Tauri's path resolver.
    pub fn resolve<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<AppPaths> {
        let p = app.path();
        Ok(Self::from_roots(
            p.data_dir()?,
            p.download_dir().ok(),
            p.home_dir()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_lives_in_muzn_dm_under_the_os_data_dir() {
        let p = AppPaths::from_roots("/data".into(), Some("/dl".into()), "/home/u".into());
        assert_eq!(p.data_dir, PathBuf::from("/data/muzn-dm"));
        assert_eq!(p.db, PathBuf::from("/data/muzn-dm/mdm.db"));
        assert_eq!(p.download_dir, PathBuf::from("/dl"));
    }

    #[test]
    fn downloads_fall_back_to_home() {
        let p = AppPaths::from_roots("/data".into(), None, "/home/u".into());
        assert_eq!(p.download_dir, PathBuf::from("/home/u"));
    }
}
