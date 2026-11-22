use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::error::Result;
use crate::store::Store;
use crate::time::{from_millis, to_millis};

impl Store {
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        let connection = self.lock()?;
        Ok(connection
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn meta_time(&self, key: &str) -> Result<Option<DateTime<Utc>>> {
        Ok(self
            .meta(key)?
            .and_then(|value| value.parse().ok())
            .map(from_millis))
    }

    pub fn set_meta_time(&self, key: &str, time: DateTime<Utc>) -> Result<()> {
        self.set_meta(key, &to_millis(time).to_string())
    }
}

/// Reads the signed-in account's `(email, display name)` without migrating or writing the database.
pub fn read_identity(database: &Path) -> Option<(String, String)> {
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let email = connection
        .query_row("SELECT value FROM meta WHERE key = 'me_email'", [], |row| {
            row.get::<_, String>(0)
        })
        .ok()?;
    let name = connection
        .query_row(
            "SELECT value FROM meta WHERE key = 'me_display_name'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()?
        .unwrap_or_default();
    Some((email, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn read_identity_returns_stored_email_and_name() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.sqlite3");
        let store = Store::open(&path).unwrap();
        store.set_meta("me_email", "ada@example.com").unwrap();
        store.set_meta("me_display_name", "Ada Lovelace").unwrap();
        drop(store);
        assert_eq!(
            read_identity(&path),
            Some(("ada@example.com".to_owned(), "Ada Lovelace".to_owned()))
        );
    }

    #[test]
    fn read_identity_missing_file_is_none() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.sqlite3");
        assert_eq!(read_identity(&path), None);
        assert!(!path.exists(), "read_identity must not create the file");
    }

    #[test]
    fn read_identity_without_email_is_none() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.sqlite3");
        let store = Store::open(&path).unwrap();
        store.set_meta("me_display_name", "Ada Lovelace").unwrap();
        drop(store);
        assert_eq!(read_identity(&path), None);
    }
}
