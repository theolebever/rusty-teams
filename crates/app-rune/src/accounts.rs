use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

const FILE: &str = "accounts.json";

/// The default folder: the registry root, and the account folder when no `--database` is given.
pub(crate) fn default_root() -> PathBuf {
    directories::ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The folder that holds a database: its parent, or the default folder for a bare file name.
pub(crate) fn account_folder(database: &Path) -> PathBuf {
    database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map_or_else(default_root, Path::to_path_buf)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Account {
    pub folder: PathBuf,
    pub email: String,
    pub name: String,
    /// Unix seconds.
    pub last_used: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Accounts {
    pub list: Vec<Account>,
    /// Set when an unreadable file could not be moved aside, so `save` must leave it alone.
    keep_file: bool,
}

impl Accounts {
    pub(crate) fn load(root: &Path) -> Accounts {
        let path = root.join(FILE);
        let (mut list, fresh, keep_file) = match fs::read_to_string(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => (Vec::new(), true, false),
            Ok(text) => match serde_json::from_str(&text) {
                Ok(list) => (list, false, false),
                Err(_) => (Vec::new(), false, !set_aside(&path)),
            },
            Err(_) => (Vec::new(), false, !set_aside(&path)),
        };
        if fresh {
            list.extend(siblings(root));
        }
        if !list.iter().any(|account| account.folder == root) {
            list.insert(0, blank(root.to_path_buf(), 0));
        }
        list.retain(|account| account.folder == root || account.folder.exists());
        Accounts { list, keep_file }
    }

    pub(crate) fn save(&self, root: &Path) -> io::Result<()> {
        if self.keep_file {
            return Ok(());
        }
        let tmp = root.join(format!("{FILE}.tmp"));
        let json = serde_json::to_string_pretty(&self.list).map_err(io::Error::other)?;
        fs::write(&tmp, json)?;
        fs::rename(tmp, root.join(FILE))
    }

    pub(crate) fn current(&self) -> &Account {
        self.list
            .iter()
            .reduce(|best, account| {
                if account.last_used > best.last_used {
                    account
                } else {
                    best
                }
            })
            .expect("load always lists the root account")
    }

    pub(crate) fn touch(&mut self, folder: &Path, now: i64) {
        if let Some(account) = self
            .list
            .iter_mut()
            .find(|account| account.folder == folder)
        {
            account.last_used = now;
        } else {
            self.list.push(blank(folder.to_path_buf(), now));
        }
    }

    pub(crate) fn set_identity(&mut self, folder: &Path, email: &str, name: &str) {
        if let Some(account) = self
            .list
            .iter_mut()
            .find(|account| account.folder == folder)
        {
            account.email = email.to_owned();
            account.name = name.to_owned();
        }
    }
}

pub(crate) fn new_folder(root: &Path) -> io::Result<PathBuf> {
    let folder = root.join("accounts").join(Uuid::new_v4().to_string());
    fs::create_dir_all(&folder)?;
    Ok(folder)
}

/// Restarts this exe with its arguments minus `--database`, so the picker's choice wins.
/// Only spawns; the caller quits the current window.
pub(crate) fn relaunch(args: impl IntoIterator<Item = OsString>) -> io::Result<()> {
    std::process::Command::new(std::env::current_exe()?)
        .args(relaunch_arguments(args))
        .env(
            app_base::updater::RELAUNCH_ENV,
            std::process::id().to_string(),
        )
        .spawn()?;
    Ok(())
}

fn relaunch_arguments(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut kept = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--database" {
            args.next();
        } else {
            kept.push(arg);
        }
    }
    kept
}

/// Renames an unreadable registry to `accounts.json.bak`. Returns false, and records why, if it could not.
fn set_aside(path: &Path) -> bool {
    let bak = path.with_file_name(format!("{FILE}.bak"));
    match fs::rename(path, &bak) {
        Ok(()) => true,
        Err(error) => {
            app_backend::diagnostics::record(format!(
                "could not keep {} as {}: {error}",
                path.display(),
                bak.display()
            ));
            false
        }
    }
}

fn blank(folder: PathBuf, last_used: i64) -> Account {
    Account {
        folder,
        email: String::new(),
        name: String::new(),
        last_used,
    }
}

/// Sibling `rusty-teams-*` profiles next to the default `ms-teams-linux` folder, found from `root`.
fn siblings(root: &Path) -> Vec<Account> {
    let Some(local) = root.parent().and_then(Path::parent) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(local) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("rusty-teams-"))
                && path.join("cache.sqlite3").is_file()
        })
        .collect();
    folders.sort();
    folders.into_iter().map(|folder| blank(folder, 0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `base/Local/ms-teams-linux/data` as the root, so siblings are looked up in `base/Local`.
    fn layout() -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let root = base.join("Local").join("ms-teams-linux").join("data");
        fs::create_dir_all(&root).unwrap();
        (base, root)
    }

    #[test]
    fn missing_file_lists_only_root() {
        let (base, root) = layout();
        assert_eq!(Accounts::load(&root).list, vec![blank(root.clone(), 0)]);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn touch_makes_current() {
        let (base, root) = layout();
        let other = base.join("other");
        let mut accounts = Accounts::load(&root);
        accounts.touch(&root, 100);
        accounts.touch(&other, 200);
        assert_eq!(accounts.current().folder, other);
        accounts.touch(&root, 300);
        assert_eq!(accounts.current().folder, root);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn round_trip() {
        let (base, root) = layout();
        let mut accounts = Accounts::load(&root);
        accounts.touch(&root, 5);
        accounts.set_identity(&root, "ada@example.com", "Ada");
        accounts.save(&root).unwrap();
        assert_eq!(Accounts::load(&root), accounts);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn missing_folder_is_dropped() {
        let (base, root) = layout();
        let gone = base.join("gone");
        fs::create_dir_all(&gone).unwrap();
        let mut accounts = Accounts::load(&root);
        accounts.touch(&gone, 1);
        accounts.save(&root).unwrap();
        fs::remove_dir_all(&gone).unwrap();
        assert_eq!(Accounts::load(&root).list, vec![blank(root.clone(), 0)]);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn corrupt_file_is_kept_as_bak() {
        let (base, root) = layout();
        fs::write(root.join(FILE), "{").unwrap();
        assert_eq!(Accounts::load(&root).list, vec![blank(root.clone(), 0)]);
        assert_eq!(
            fs::read_to_string(root.join("accounts.json.bak")).unwrap(),
            "{"
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn unreadable_file_is_kept_as_bak() {
        let (base, root) = layout();
        fs::write(root.join(FILE), [0xff, 0xfe]).unwrap();
        assert_eq!(Accounts::load(&root).list, vec![blank(root.clone(), 0)]);
        assert_eq!(
            fs::read(root.join("accounts.json.bak")).unwrap(),
            [0xff, 0xfe]
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn file_that_cannot_be_moved_aside_is_not_overwritten() {
        let (base, root) = layout();
        // A directory at the backup path makes the rename fail on Windows and Unix alike.
        fs::create_dir_all(root.join("accounts.json.bak")).unwrap();
        fs::write(root.join(FILE), "{").unwrap();
        let accounts = Accounts::load(&root);
        accounts.save(&root).unwrap();
        assert_eq!(fs::read_to_string(root.join(FILE)).unwrap(), "{");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn new_folder_is_created_under_root() {
        let (base, root) = layout();
        let first = new_folder(&root).unwrap();
        assert!(first.is_dir());
        assert_eq!(first.parent(), Some(root.join("accounts").as_path()));
        assert_ne!(first, new_folder(&root).unwrap());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn relaunch_arguments_drop_database() {
        let args = ["--database", "x", "--sso"].map(OsString::from);
        assert_eq!(relaunch_arguments(args), vec![OsString::from("--sso")]);
    }

    #[test]
    fn first_load_adopts_rusty_teams_folders() {
        let (base, root) = layout();
        let secondary = base.join("Local").join("rusty-teams-secondary");
        fs::create_dir_all(&secondary).unwrap();
        fs::write(secondary.join("cache.sqlite3"), "").unwrap();
        let folders: Vec<PathBuf> = Accounts::load(&root)
            .list
            .into_iter()
            .map(|account| account.folder)
            .collect();
        assert_eq!(folders, vec![root, secondary]);
        fs::remove_dir_all(&base).unwrap();
    }
}
