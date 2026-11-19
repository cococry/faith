use super::{Chat, Message, Metadata};
use faith_client::{EventStore, GroupKind, GroupPacket, Id, StoredEvent};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::Mutex,
};

#[derive(Clone, Debug)]
pub struct Conversation {
    pub revision: u64,
    pub members: Vec<Id>,
    pub active: bool,
}

pub struct Store {
    db: Mutex<Connection>,
    _profile_lock: File,
}

fn id(bytes: Vec<u8>) -> rusqlite::Result<Id> {
    bytes
        .try_into()
        .map(Id)
        .map_err(|_| rusqlite::Error::InvalidQuery)
}

fn members(bytes: Vec<u8>) -> rusqlite::Result<Vec<Id>> {
    if bytes.is_empty()
        || !bytes.len().is_multiple_of(16)
        || bytes.len() > 16 * faith_client::MAX_MEMBERS
    {
        return Err(rusqlite::Error::InvalidQuery);
    }

    Ok(bytes.as_chunks::<16>().0.iter().map(|b| Id(*b)).collect())
}

fn pack(ids: &[Id]) -> Vec<u8> {
    ids.iter().flat_map(|id| id.0).collect()
}

fn conversation(db: &Connection, group: Id) -> rusqlite::Result<Option<Conversation>> {
    db.query_row(
        "SELECT revision,members,active FROM conversations WHERE id=?",
        [group.0.as_slice()],
        |r| {
            Ok(Conversation {
                revision: r.get(0)?,
                members: members(r.get(1)?)?,
                active: r.get(2)?,
            })
        },
    )
    .optional()
}

fn select(db: &Connection, group: Id) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO settings(key,value) VALUES('selected',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [group.0.as_slice()],
    )?;
    Ok(())
}

fn chat_title(id: Id, metadata: Option<&Metadata>) -> String {
    metadata.map_or_else(
        || format!("Chat · {}", &id.to_string()[..8]),
        |m| m.title.clone(),
    )
}

pub fn conversation_titles(profile: &Path) -> anyhow::Result<Vec<(Id, String)>> {
    let directory = fs::symlink_metadata(profile)?;
    let database = profile.join("conversations.sqlite");
    let metadata = match fs::symlink_metadata(&database) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let uid = unsafe { nix::libc::geteuid() };
    anyhow::ensure!(
        directory.is_dir()
            && directory.uid() == uid
            && directory.mode() & 0o077 == 0
            && metadata.is_file()
            && metadata.uid() == uid
            && metadata.mode() & 0o077 == 0,
        "Conversation storage is unavailable"
    );
    let db = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    db.busy_timeout(std::time::Duration::ZERO)?;
    let mut stmt = db.prepare(
        "SELECT c.id, m.data FROM conversations c
         LEFT JOIN chat_metadata m ON m.conversation=c.id
         WHERE c.active ORDER BY c.rowid DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((id(row.get(0)?)?, row.get::<_, Option<String>>(1)?))
    })?;
    rows.map(|row| {
        let (id, metadata) = row?;
        let metadata = metadata.as_deref().map(serde_json::from_str).transpose()?;
        Ok((id, chat_title(id, metadata.as_ref())))
    })
    .collect()
}

impl Store {
    pub fn open(profile: &Path) -> anyhow::Result<Self> {
        if !profile.exists() {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(profile)?;
        }

        let metadata = fs::symlink_metadata(profile)?;
        anyhow::ensure!(
            metadata.is_dir()
                && metadata.uid() == unsafe { nix::libc::geteuid() }
                && metadata.mode() & 0o077 == 0,
            "profile must be an owned directory with permissions 0700"
        );
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(profile.join("profile.lock"))?;
        lock.try_lock()
            .map_err(|_| anyhow::anyhow!("profile is already open by another client"))?;
        let database = profile.join("conversations.sqlite");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(&database)?;
        anyhow::ensure!(
            file.metadata()?.is_file() && file.metadata()?.uid() == metadata.uid(),
            "database must be an owned regular file"
        );
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        let db = Connection::open(database)?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS conversations(id BLOB PRIMARY KEY,revision INTEGER NOT NULL,members BLOB NOT NULL,active INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS messages(conversation BLOB NOT NULL,sender BLOB NOT NULL,id BLOB NOT NULL,body TEXT NOT NULL,PRIMARY KEY(conversation,sender,id));
            CREATE TABLE IF NOT EXISTS cursors(device BLOB PRIMARY KEY,seq INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS outbox(id BLOB PRIMARY KEY,packet BLOB NOT NULL,targets BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS chat_metadata(conversation BLOB PRIMARY KEY,data TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS messages_by_conversation ON messages(conversation);")?;
        Ok(Self {
            db: Mutex::new(db),
            _profile_lock: lock,
        })
    }

    fn db(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Connection>> {
        self.db
            .lock()
            .map_err(|_| anyhow::anyhow!("local storage lock poisoned"))
    }

    pub fn conversation(&self, group: Id) -> anyhow::Result<Option<Conversation>> {
        Ok(conversation(&*self.db()?, group)?)
    }

    pub fn selected(&self) -> anyhow::Result<Option<Id>> {
        let db = self.db()?;
        let bytes: Option<Vec<u8>> = db
            .query_row("SELECT value FROM settings WHERE key='selected'", [], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(bytes.map(id).transpose()?)
    }

    pub fn select(&self, group: Id) -> anyhow::Result<()> {
        select(&*self.db()?, group)?;
        Ok(())
    }

    pub fn chats(&self) -> anyhow::Result<Vec<Chat>> {
        let db = self.db()?;
        let mut stmt = db.prepare_cached(
            "SELECT c.id, m.data, (SELECT body FROM messages WHERE conversation=c.id ORDER BY rowid DESC LIMIT 1)
             FROM conversations c LEFT JOIN chat_metadata m ON m.conversation=c.id
             WHERE c.active ORDER BY c.rowid DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                id(row.get(0)?)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut chats = Vec::new();

        for row in rows {
            let (id, metadata, last) = row?;
            let metadata: Option<Metadata> =
                metadata.as_deref().map(serde_json::from_str).transpose()?;
            chats.push(Chat {
                id,
                needs_name: metadata.is_none(),
                title: chat_title(id, metadata.as_ref()),
                last: last.map_or_else(
                    || "No messages yet".into(),
                    |body| {
                        let preview: String = body
                            .chars()
                            .take(120)
                            .map(|c| {
                                if c.is_control() {
                                    ' '
                                } else {
                                    c
                                }
                            })
                            .collect();
                        preview
                    },
                ),
                metadata,
            });
        }

        Ok(chats)
    }

    pub fn rename(&self, id: Id, auth: Id, title: String) -> anyhow::Result<()> {
        let db = self.db()?;
        let chat = conversation(&db, id)?
            .filter(|chat| chat.active)
            .ok_or_else(|| anyhow::anyhow!("Conversation is unavailable"))?;
        let existing: Option<String> = db
            .query_row(
                "SELECT data FROM chat_metadata WHERE conversation=?",
                [id.0.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        let mut metadata = match existing {
            Some(data) => serde_json::from_str(&data)?,
            None => Metadata {
                title: String::new(),
                partners: Vec::new(),
            },
        };
        // in a direct chat the other member can use the same name as the conversation
        if chat.members.len() == 2
            && let Some(partner) = chat.members.iter().find(|id| **id != auth)
            && metadata.partners.is_empty()
        {
            metadata.partners = vec![(partner.0, title.clone())];
        }

        metadata.title = title;
        db.execute(
            "INSERT INTO chat_metadata(conversation,data) VALUES(?,?) ON CONFLICT(conversation) DO UPDATE SET data=excluded.data",
            params![id.0.as_slice(), serde_json::to_string(&metadata)?],
        )?;
        Ok(())
    }

    pub fn history(&self, chat: &Chat) -> anyhow::Result<Vec<Message>> {
        let db = self.db()?;
        let mut stmt = db.prepare_cached(
            "SELECT sender,body,rowid FROM messages WHERE conversation=? ORDER BY rowid DESC LIMIT 100",
        )?;
        let mut messages = stmt
            .query_map([chat.id.0.as_slice()], |row| {
                let sender = id(row.get(0)?)?;
                let body: String = row.get(1)?;
                let author = chat
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.partners.iter().find(|(id, _)| *id == sender.0))
                    .map_or_else(|| sender.to_string(), |(_, name)| name.clone());
                Ok(Message {
                    row_id: row.get(2)?,
                    sender,
                    author,
                    body: body
                        .chars()
                        .filter(|c| !c.is_control() || *c == '\n')
                        .collect(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        messages.reverse();
        Ok(messages)
    }

    pub fn pending(&self) -> anyhow::Result<Option<(GroupPacket, Vec<Id>)>> {
        let db = self.db()?;
        let row: Option<(Vec<u8>, Vec<u8>)> = db
            .query_row("SELECT packet,targets FROM outbox LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        row.map(|(packet, targets)| Ok((GroupPacket::decode(&packet)?, members_targets(targets)?)))
            .transpose()
    }

    pub fn enqueue(
        &self,
        packet: &GroupPacket,
        targets: &[Id],
        metadata: Option<&Metadata>,
    ) -> anyhow::Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        anyhow::ensure!(
            !tx.query_row("SELECT EXISTS(SELECT 1 FROM outbox)", [], |r| r
                .get::<_, bool>(0))?,
            "a delivery is pending; use /retry or /discard"
        );
        tx.execute(
            "INSERT INTO outbox(id,packet,targets) VALUES(?,?,?)",
            params![packet.message.0.as_slice(), packet.encode()?, pack(targets)],
        )?;
        if let Some(metadata) = metadata {
            tx.execute(
                "INSERT INTO chat_metadata(conversation,data) VALUES(?,?)",
                params![
                    packet.conversation.0.as_slice(),
                    serde_json::to_string(metadata)?
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn discard_pending(&self) -> anyhow::Result<()> {
        self.db()?.execute("DELETE FROM outbox", [])?;
        Ok(())
    }

    fn apply(&self, event: StoredEvent<'_>) -> anyhow::Result<()> {
        anyhow::ensure!(
            event.sequence <= i64::MAX as u64,
            "invalid delivery sequence"
        );
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let previous: Option<u64> = tx
            .query_row(
                "SELECT seq FROM cursors WHERE device=?",
                [event.device.0.as_slice()],
                |r| r.get(0),
            )
            .optional()?;

        if previous.is_some_and(|seq| event.sequence <= seq) {
            return Ok(());
        }

        if event.kind == faith_client::GROUP_PACKET_EVENT {
            anyhow::ensure!(event.data.len() >= 16, "truncated group event");
            let sender = Id(event.data[..16].try_into()?);
            let packet = GroupPacket::decode(&event.data[16..])?;
            let old = conversation(&tx, packet.conversation)?;

            let accepted = match packet.kind {
                GroupKind::Message => old.as_ref().is_some_and(|old| {
                    old.active
                        && old.revision == packet.revision
                        && old.members == packet.members
                        && old.members.contains(&sender)
                        && old.members.contains(&event.auth)
                }),
                _ => {
                    packet.members[0] == sender
                        && match old.as_ref() {
                            None => packet.members.contains(&event.auth),
                            Some(old) => old.members[0] == sender && packet.revision > old.revision,
                        }
                }
            };

            if accepted
                && (packet.kind != GroupKind::Message || std::str::from_utf8(&packet.text).is_ok())
            {
                if packet.kind == GroupKind::Message {
                    let text = std::str::from_utf8(&packet.text)?;
                    tx.execute("INSERT OR IGNORE INTO messages(conversation,sender,id,body) VALUES(?,?,?,?)",
                        params![packet.conversation.0.as_slice(),sender.0.as_slice(),packet.message.0.as_slice(),text])?;
                } else {
                    tx.execute("INSERT INTO conversations(id,revision,members,active) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,members=excluded.members,active=excluded.active",
                        params![packet.conversation.0.as_slice(),packet.revision,pack(&packet.members),packet.members.contains(&event.auth)])?;
                }

                if sender == event.auth {
                    tx.execute(
                        "DELETE FROM outbox WHERE id=?",
                        [packet.message.0.as_slice()],
                    )?;
                }
            }
            // Rejected membership/forged sender is durably handled, never applied.
        }

        tx.execute("INSERT INTO cursors(device,seq) VALUES(?,?) ON CONFLICT(device) DO UPDATE SET seq=excluded.seq",params![event.device.0.as_slice(),event.sequence])?;
        tx.commit()?;
        Ok(())
    }
}

fn members_targets(bytes: Vec<u8>) -> anyhow::Result<Vec<Id>> {
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len().is_multiple_of(16) && bytes.len() <= 64 * 16,
        "invalid outbox targets"
    );
    Ok(bytes.as_chunks::<16>().0.iter().map(|b| Id(*b)).collect())
}

impl EventStore for Store {
    fn store(&self, event: StoredEvent<'_>) -> faith_client::Result<()> {
        self.apply(event).map_err(|error| {
            tracing::error!("Durable event storage failed: {error}");
            faith_client::Error::Native(6)
        })
    }
}
