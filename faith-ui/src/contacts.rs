use faith_client::Id;
use rusqlite::{Connection, OpenFlags, params};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub const ICON: &str = "󰛋";
pub const EDIT: &str = "";
pub const DELETE: &str = "󰆴";
pub const NAME_MAX: usize = 20;

pub struct Contact {
    pub id: Id,
    pub auth: String,
    pub name: String,
    pub display: String,
}

pub struct Book {
    db: Connection,
    pub entries: Vec<Contact>,
    names: HashMap<Id, usize>,
}

impl Book {
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("contacts.sqlite");
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&path)?;
        let metadata = file.metadata()?;
        anyhow::ensure!(
            metadata.is_file() && metadata.uid() == unsafe { nix::libc::geteuid() },
            "Contacts storage must be an owned regular file"
        );
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(2))?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS contacts(auth BLOB PRIMARY KEY, alias TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS imported_profiles(path BLOB PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS deleted_contacts(auth BLOB PRIMARY KEY);",
        )?;
        let mut book = Self {
            db,
            entries: Vec::new(),
            names: HashMap::new(),
        };
        book.reload()?;
        Ok(book)
    }

    pub fn import(&mut self, profile: &Path) -> anyhow::Result<()> {
        let path = match profile.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let key = path.as_os_str().as_encoded_bytes();
        let imported: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM imported_profiles WHERE path=?)",
            [key],
            |row| row.get(0),
        )?;
        if imported {
            return Ok(());
        }

        let Some(aliases) = crate::client::legacy_aliases(profile)? else {
            return Ok(());
        };

        let tx = self.db.transaction()?;
        let first = tx.execute("INSERT OR IGNORE INTO imported_profiles VALUES(?)", [key])?;
        if first != 0 {
            for (id, name) in aliases {
                let name = name.trim();
                if !name.is_empty() && !name.chars().any(char::is_control) {
                    tx.execute(
                        "INSERT OR IGNORE INTO contacts
                         SELECT ?1,?2 WHERE NOT EXISTS (SELECT 1 FROM deleted_contacts WHERE auth=?1)",
                        params![id.0, name],
                    )?;
                }
            }
        }

        tx.commit()?;
        self.reload()
    }

    pub fn save(&mut self, edits: &[(Id, String)]) -> anyhow::Result<()> {
        let tx = self.db.transaction()?;
        for (id, name) in edits {
            let name = name.trim();
            anyhow::ensure!(
                !name.is_empty()
                    && name.chars().count() <= NAME_MAX
                    && !name.chars().any(char::is_control),
                "Use 1–{NAME_MAX} characters for the local name"
            );
            tx.execute(
                "INSERT INTO contacts VALUES(?,?) ON CONFLICT(auth) DO UPDATE SET alias=excluded.alias",
                params![id.0, name],
            )?;
        }

        tx.commit()?;
        self.reload()
    }

    pub fn delete(&mut self, id: Id) -> anyhow::Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("INSERT OR IGNORE INTO deleted_contacts VALUES(?)", [id.0])?;
        tx.execute("DELETE FROM contacts WHERE auth=?", [id.0])?;
        tx.commit()?;
        self.reload()
    }

    pub fn get(&self, id: Id) -> Option<&Contact> {
        self.names.get(&id).map(|&index| &self.entries[index])
    }

    pub fn display<'a>(&'a self, id: Id, fallback: &'a str) -> &'a str {
        self.get(id)
            .map_or(fallback, |contact| contact.display.as_str())
    }

    pub fn apply_usernames(&mut self, usernames: &HashMap<Id, String>) {
        for contact in &mut self.entries {
            contact.display = usernames.get(&contact.id).map_or_else(
                || contact.name.clone(),
                |username| format!("{} ({username})", contact.name),
            );
        }
    }

    fn reload(&mut self) -> anyhow::Result<()> {
        let mut stmt = self.db.prepare_cached(
            "SELECT auth,alias FROM contacts ORDER BY alias COLLATE NOCASE, auth",
        )?;
        self.entries = stmt
            .query_map([], |row| {
                let id = Id(row.get(0)?);
                let name: String = row.get(1)?;
                Ok(Contact {
                    id,
                    auth: id.to_string(),
                    display: name.clone(),
                    name,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        self.names = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| (entry.id, i))
            .collect();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn imports_aliases_across_profiles_once_and_persists_edits() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!("faith-contacts-{}", Id::random()?));
        fs::create_dir(&root)?;
        let result = (|| -> anyhow::Result<()> {
            let bob = Id([1; 16]);
            let alice = Id([2; 16]);
            let other = Id([3; 16]);
            let first = root.join("First");
            let second = root.join("Second");
            let third = root.join("Third");
            for (profile, aliases) in [
                (&first, vec![(bob.0, "Bob"), (alice.0, "Alice")]),
                (&second, vec![(bob.0, "Bobby"), (other.0, "Charlie")]),
                (&third, vec![(alice.0, "Ally")]),
            ] {
                fs::create_dir(profile)?;
                fs::set_permissions(profile, fs::Permissions::from_mode(0o700))?;
                let path = profile.join("conversations.sqlite");
                let db = Connection::open(&path)?;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
                db.execute("CREATE TABLE chat_metadata(data TEXT NOT NULL)", [])?;
                db.execute(
                    "INSERT INTO chat_metadata VALUES(?)",
                    [serde_json::json!({"title": "", "partners": aliases}).to_string()],
                )?;
            }

            let mut book = Book::open(&root)?;
            book.import(&first)?;
            book.import(&second)?;
            assert_eq!(book.entries.len(), 3);
            assert_eq!(book.get(bob).unwrap().name, "Bob");
            book.save(&[(bob, "Robert".into())])?;
            book.apply_usernames(&HashMap::from([(bob, "bobthekiller69".into())]));
            assert_eq!(book.display(bob, ""), "Robert (bobthekiller69)");
            book.delete(alice)?;
            drop(book);

            let mut reopened = Book::open(&root)?;
            reopened.import(&first)?;
            reopened.import(&second)?;
            reopened.import(&third)?;
            assert_eq!(reopened.entries.len(), 2);
            assert_eq!(reopened.get(bob).unwrap().name, "Robert");
            assert!(reopened.get(alice).is_none());
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(&root);
        result?;
        cleanup?;
        Ok(())
    }
}
