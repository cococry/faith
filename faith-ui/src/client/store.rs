use super::{Chat, InvitationPolicy, Message, Metadata, Request};
use faith_client::{EventStore, GroupKind, GroupPacket, Id, StoredEvent};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    collections::HashMap,
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

pub struct InvitationBatch {
    pub policy: InvitationPolicy,
    pub responses: Vec<(Id, bool)>,
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

fn chat_title(
    db: &Connection,
    id: Id,
    metadata: Option<&Metadata>,
    chat_members: &[Id],
    own_auth: Option<Id>,
) -> anyhow::Result<String> {
    if let Some(metadata) = metadata.filter(|m| !title_members(Some(m), chat_members, own_auth)) {
        return Ok(metadata.title.clone());
    }

    let mut stmt =
        db.prepare_cached("SELECT member,username FROM disclosed_usernames WHERE conversation=?")?;
    let names: HashMap<Id, String> = stmt
        .query_map([id.0], |row| Ok((self::id(row.get(0)?)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut disclosed = chat_members
        .iter()
        .filter(|&&member| Some(member) != own_auth)
        .filter_map(|member| names.get(member).map(String::as_str));
    let first: Vec<_> = disclosed.by_ref().take(3).collect();
    if first.is_empty() {
        if metadata.is_some_and(|m| m.automatic_title) {
            let mut partners = chat_members
                .iter()
                .filter(|&&member| Some(member) != own_auth);
            let mut title = partners
                .by_ref()
                .take(3)
                .map(|id| format!("{}…", &id.to_string()[..4]))
                .collect::<Vec<_>>()
                .join(", ");
            if partners.next().is_some() {
                title.push_str(" & more");
            }

            if !title.is_empty() {
                return Ok(title);
            }
        }

        return Ok(format!("Chat · {}", &id.to_string()[..8]));
    }

    let mut title = first.join(", ");
    let remaining = disclosed.count();
    if remaining != 0 {
        title.push_str(&format!(" & {remaining} more"));
    }

    Ok(title)
}

fn open_reader(profile: &Path) -> anyhow::Result<Option<Connection>> {
    let directory = fs::symlink_metadata(profile)?;
    let database = profile.join("conversations.sqlite");
    let metadata = match fs::symlink_metadata(&database) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
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
    Ok(Some(db))
}

fn has_table(db: &Connection, name: &str) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?)",
        [name],
        |row| row.get(0),
    )
}

pub fn legacy_aliases(profile: &Path) -> anyhow::Result<Option<Vec<(Id, String)>>> {
    let Some(db) = open_reader(profile)? else {
        return Ok(None);
    };

    if !has_table(&db, "chat_metadata")? {
        return Ok(Some(Vec::new()));
    }

    let mut stmt = db.prepare("SELECT data FROM chat_metadata ORDER BY rowid DESC")?;
    let mut aliases = Vec::new();
    for row in stmt.query_map([], |row| row.get::<_, String>(0))? {
        let metadata: Metadata = serde_json::from_str(&row?)?;
        aliases.extend(
            metadata
                .partners
                .into_iter()
                .map(|(auth, name)| (Id(auth), name)),
        );
    }

    Ok(Some(aliases))
}

fn usernames(db: &Connection) -> anyhow::Result<Vec<(Id, String)>> {
    let mut stmt = db.prepare_cached(if has_table(db, "incoming_requests")? {
        "SELECT member,username FROM (
            SELECT requester AS member, username, 0 AS source, rowid FROM incoming_requests WHERE username != ''
            UNION ALL
            SELECT member, username, 1 AS source, rowid FROM disclosed_usernames
        ) ORDER BY source, rowid"
    } else {
        "SELECT member,username FROM disclosed_usernames ORDER BY rowid"
    })?;
    Ok(stmt
        .query_map([], |row| Ok((id(row.get(0)?)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

pub fn profile_usernames(profile: &Path) -> anyhow::Result<Vec<(Id, String)>> {
    let Some(db) = open_reader(profile)? else {
        return Ok(Vec::new());
    };

    if has_table(&db, "disclosed_usernames")? {
        usernames(&db)
    } else {
        Ok(Vec::new())
    }
}

pub fn profile_conversations(profile: &Path) -> anyhow::Result<Vec<Chat>> {
    let Some(db) = open_reader(profile)? else {
        return Ok(Vec::new());
    };

    let own_auth = db
        .query_row("SELECT value FROM settings WHERE key='auth'", [], |row| {
            id(row.get(0)?)
        })
        .optional()?;
    let has_usernames = has_table(&db, "disclosed_usernames")?;
    let mut stmt = db.prepare(
        "SELECT c.id, m.data, c.members FROM conversations c
         LEFT JOIN chat_metadata m ON m.conversation=c.id
         WHERE c.active ORDER BY c.rowid DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            id(row.get(0)?)?,
            row.get::<_, Option<String>>(1)?,
            members(row.get(2)?)?,
        ))
    })?;
    rows.map(|row| {
        let (id, metadata, members) = row?;
        let metadata: Option<Metadata> =
            metadata.as_deref().map(serde_json::from_str).transpose()?;
        let title = if has_usernames {
            chat_title(&db, id, metadata.as_ref(), &members, own_auth)?
        } else {
            metadata
                .as_ref()
                .filter(|m| !title_members(Some(m), &members, own_auth))
                .map_or_else(
                    || format!("Chat · {}", &id.to_string()[..8]),
                    |metadata| metadata.title.clone(),
                )
        };
        Ok(Chat {
            id,
            title,
            needs_name: metadata
                .as_ref()
                .is_none_or(|m| m.title.is_empty() && !m.automatic_title),
            name_members: title_members(metadata.as_ref(), &members, own_auth),
            members,
            own_auth,
            last: String::new(),
        })
    })
    .collect()
}

fn title_members(metadata: Option<&Metadata>, members: &[Id], auth: Option<Id>) -> bool {
    metadata.is_none_or(|m| {
        m.automatic_title
            || m.title.is_empty()
            || m.title_from.is_some()
            || (members.len() == 2
                && m.partners
                    .iter()
                    .any(|(id, alias)| Some(Id(*id)) != auth && *alias == m.title))
    })
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
            CREATE TABLE IF NOT EXISTS incoming_requests(conversation BLOB PRIMARY KEY, requester BLOB NOT NULL, required INTEGER NOT NULL, username TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS accepted_invites(conversation BLOB PRIMARY KEY, requester BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS local_creations(conversation BLOB PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS invitation_batches(conversation BLOB PRIMARY KEY, required INTEGER NOT NULL, disclose INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS invitation_responses(conversation BLOB NOT NULL, recipient BLOB NOT NULL, accepted INTEGER NOT NULL, PRIMARY KEY(conversation,recipient));
            CREATE TABLE IF NOT EXISTS disclosed_usernames(conversation BLOB NOT NULL, member BLOB NOT NULL, username TEXT NOT NULL, PRIMARY KEY(conversation,member));
            CREATE TABLE IF NOT EXISTS chat_metadata(conversation BLOB PRIMARY KEY,data TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS messages_by_conversation ON messages(conversation);")?;
        let has_disclosure = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('invitation_batches') WHERE name='disclose')",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        if !has_disclosure {
            db.execute(
                "ALTER TABLE invitation_batches ADD COLUMN disclose INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }

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

    pub fn remember_auth(&self, auth: Id) -> anyhow::Result<()> {
        self.db()?.execute(
            "INSERT INTO settings(key,value) VALUES('auth',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE value!=excluded.value",
            [auth.0],
        )?;
        Ok(())
    }

    pub fn chats(&self, auth: Id) -> anyhow::Result<Vec<Chat>> {
        let db = self.db()?;
        let mut stmt = db.prepare_cached(
            "SELECT c.id, m.data, c.members, (SELECT body FROM messages WHERE conversation=c.id ORDER BY rowid DESC LIMIT 1)
             FROM conversations c LEFT JOIN chat_metadata m ON m.conversation=c.id
             WHERE c.active ORDER BY c.rowid DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                id(row.get(0)?)?,
                row.get::<_, Option<String>>(1)?,
                members(row.get(2)?)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut chats = Vec::new();

        for row in rows {
            let (id, metadata, members, last) = row?;
            let metadata: Option<Metadata> =
                metadata.as_deref().map(serde_json::from_str).transpose()?;
            chats.push(Chat {
                id,
                needs_name: metadata
                    .as_ref()
                    .is_none_or(|m| m.title.is_empty() && !m.automatic_title),
                title: chat_title(&db, id, metadata.as_ref(), &members, Some(auth))?,
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
                name_members: title_members(metadata.as_ref(), &members, Some(auth)),
                members,
                own_auth: Some(auth),
            });
        }

        Ok(chats)
    }

    pub fn rename(&self, id: Id, title: String) -> anyhow::Result<()> {
        let db = self.db()?;
        anyhow::ensure!(
            conversation(&db, id)?.is_some_and(|chat| chat.active),
            "Conversation is unavailable"
        );
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
                title_from: None,
                automatic_title: false,
                partners: Vec::new(),
            },
        };
        metadata.title = title;
        metadata.title_from = None;
        metadata.automatic_title = false;
        metadata.partners.clear();
        db.execute(
            "INSERT INTO chat_metadata(conversation,data) VALUES(?,?) ON CONFLICT(conversation) DO UPDATE SET data=excluded.data",
            params![id.0.as_slice(), serde_json::to_string(&metadata)?],
        )?;
        Ok(())
    }

    pub fn usernames(&self) -> anyhow::Result<Vec<(Id, String)>> {
        usernames(&*self.db()?)
    }

    pub fn history(&self, chat: &Chat) -> anyhow::Result<Vec<Message>> {
        let db = self.db()?;
        let mut stmt = db.prepare_cached(
            "SELECT m.sender,m.body,m.rowid,u.username FROM messages m
             LEFT JOIN disclosed_usernames u ON u.conversation=m.conversation AND u.member=m.sender
             WHERE m.conversation=? ORDER BY m.rowid DESC LIMIT 100",
        )?;
        let mut messages = stmt
            .query_map([chat.id.0.as_slice()], |row| {
                let sender = id(row.get(0)?)?;
                let body: String = row.get(1)?;
                let disclosed: Option<String> = row.get(3)?;
                Ok(Message {
                    row_id: row.get(2)?,
                    sender,
                    author: disclosed.unwrap_or_else(|| sender.to_string()),
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

    pub fn requests(&self) -> anyhow::Result<Vec<Request>> {
        let db = self.db()?;
        let mut stmt = db.prepare_cached(
            "SELECT r.conversation,r.requester,r.required,COALESCE(u.username,r.username)
             FROM incoming_requests r LEFT JOIN disclosed_usernames u
             ON u.conversation=r.conversation AND u.member=r.requester ORDER BY r.rowid",
        )?;
        Ok(stmt
            .query_map([], |row| {
                Ok(Request {
                    conversation: id(row.get(0)?)?,
                    requester: id(row.get(1)?)?,
                    required: row.get(2)?,
                    username: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn remove_request(&self, conversation: Id) -> anyhow::Result<()> {
        self.db()?.execute(
            "DELETE FROM incoming_requests WHERE conversation=?",
            [conversation.0],
        )?;
        Ok(())
    }

    pub fn accept_request(&self, conversation: Id, requester: Id, auth: Id) -> anyhow::Result<()> {
        let db = self.db()?;
        let tx = db.unchecked_transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO conversations(id,revision,members,active) VALUES(?,0,?,1)",
            params![conversation.0.as_slice(), pack(&[requester, auth])],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO accepted_invites(conversation,requester) VALUES(?,?)",
            params![conversation.0.as_slice(), requester.0.as_slice()],
        )?;
        tx.execute(
            "DELETE FROM incoming_requests WHERE conversation=?",
            [conversation.0],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn invitations(&self, conversation: Id) -> anyhow::Result<Option<InvitationBatch>> {
        let db = self.db()?;
        let policy = db
            .query_row(
                "SELECT required,disclose FROM invitation_batches WHERE conversation=?",
                [conversation.0],
                |row| {
                    Ok(InvitationPolicy {
                        required: row.get(0)?,
                        disclose: row.get(1)?,
                    })
                },
            )
            .optional()?;
        let Some(policy) = policy else {
            return Ok(None);
        };

        let mut stmt = db.prepare_cached(
            "SELECT recipient,accepted FROM invitation_responses WHERE conversation=?",
        )?;
        let responses = stmt
            .query_map([conversation.0], |row| Ok((id(row.get(0)?)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Some(InvitationBatch { policy, responses }))
    }

    pub fn pending_invites(&self, auth: Id) -> anyhow::Result<Vec<super::PendingInvite>> {
        let db = self.db()?;
        let mut stmt = db.prepare(
            "SELECT b.conversation,o.packet,o.targets,m.data
             FROM invitation_batches b CROSS JOIN outbox o
             LEFT JOIN chat_metadata m ON m.conversation=b.conversation",
        )?;
        let batches = stmt
            .query_map([], |row| {
                Ok((
                    id(row.get(0)?)?,
                    row.get::<_, Vec<u8>>(1)?,
                    members(row.get(2)?)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut invites = Vec::new();
        for (conversation, packet, targets, metadata) in batches {
            let packet = GroupPacket::decode(&packet)?;
            if packet.conversation != conversation {
                continue;
            }
            let metadata: Option<Metadata> =
                metadata.as_deref().map(serde_json::from_str).transpose()?;
            let title = chat_title(
                &db,
                conversation,
                metadata.as_ref(),
                &packet.members,
                Some(auth),
            )?;
            let title_members =
                title_members(metadata.as_ref(), &packet.members, Some(auth)).then(|| {
                    packet
                        .members
                        .iter()
                        .copied()
                        .filter(|&id| id != auth)
                        .collect::<std::sync::Arc<[_]>>()
                });
            for recipient in targets.into_iter().filter(|target| *target != auth) {
                let accepted = db
                    .query_row(
                        "SELECT accepted FROM invitation_responses WHERE conversation=? AND recipient=?",
                        params![conversation.0, recipient.0],
                        |row| row.get::<_, bool>(0),
                    )
                    .optional()?;
                let recipient_name = format!("{}…", &recipient.to_string()[..8]);
                invites.push(super::PendingInvite {
                    conversation,
                    title: title.clone(),
                    title_members: title_members.clone(),
                    recipient,
                    recipient_name,
                    accepted,
                });
            }
        }
        Ok(invites)
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
        invite: Option<InvitationPolicy>,
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

        if packet.kind == GroupKind::Create {
            tx.execute(
                "INSERT INTO conversations(id,revision,members,active) VALUES(?,?,?,1)",
                params![
                    packet.conversation.0,
                    packet.revision,
                    pack(&packet.members)
                ],
            )?;
            tx.execute(
                "INSERT INTO local_creations(conversation) VALUES(?)",
                [packet.conversation.0],
            )?;
        }

        if let Some(policy) = invite {
            tx.execute(
                "INSERT INTO invitation_batches(conversation,required,disclose) VALUES(?,?,?)",
                params![packet.conversation.0, policy.required, policy.disclose],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn discard_pending(&self) -> anyhow::Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        tx.execute("DELETE FROM outbox", [])?;
        tx.execute("DELETE FROM invitation_batches", [])?;
        tx.execute("DELETE FROM invitation_responses", [])?;
        tx.execute(
            "DELETE FROM chat_metadata WHERE conversation IN (SELECT conversation FROM local_creations)",
            [],
        )?;
        tx.execute(
            "DELETE FROM conversations WHERE id IN (SELECT conversation FROM local_creations)",
            [],
        )?;
        tx.execute("DELETE FROM local_creations", [])?;
        tx.commit()?;
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
            let accepted_invite: Option<Id> = tx
                .query_row(
                    "SELECT requester FROM accepted_invites WHERE conversation=?",
                    [packet.conversation.0.as_slice()],
                    |row| id(row.get(0)?),
                )
                .optional()?;
            let local_creation: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM local_creations WHERE conversation=?)",
                [packet.conversation.0.as_slice()],
                |row| row.get(0),
            )?;

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
                            Some(_) if accepted_invite.is_some() => {
                                accepted_invite == Some(sender)
                                    && packet.members.contains(&event.auth)
                            }
                            Some(old) if local_creation => {
                                sender == event.auth
                                    && old.revision == packet.revision
                                    && old.members == packet.members
                            }
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
                    tx.execute(
                        "DELETE FROM incoming_requests WHERE conversation=?",
                        [packet.conversation.0],
                    )?;
                    tx.execute(
                        "DELETE FROM accepted_invites WHERE conversation=?",
                        [packet.conversation.0],
                    )?;
                    tx.execute(
                        "DELETE FROM local_creations WHERE conversation=?",
                        [packet.conversation.0],
                    )?;
                }

                if sender == event.auth {
                    let removed = tx.execute(
                        "DELETE FROM outbox WHERE id=?",
                        [packet.message.0.as_slice()],
                    )?;
                    if removed != 0 && packet.kind == GroupKind::Create {
                        tx.execute(
                            "DELETE FROM invitation_batches WHERE conversation=?",
                            [packet.conversation.0],
                        )?;
                        tx.execute(
                            "DELETE FROM invitation_responses WHERE conversation=?",
                            [packet.conversation.0],
                        )?;
                    }
                }
            }
            // Rejected membership/forged sender is durably handled, never applied.
        } else if matches!(
            event.kind,
            super::REQUEST_EVENT | super::RESPONSE_EVENT | super::USERNAMES_EVENT
        ) {
            // The native client validates these payloads before handing them to us.
            let conversation = &event.data[..16];
            match event.kind {
                super::REQUEST_EVENT => {
                    let required = event.data.len() > 32;
                    let username = if required {
                        std::str::from_utf8(&event.data[33..])?
                    } else {
                        ""
                    };
                    tx.execute(
                        "INSERT OR IGNORE INTO incoming_requests VALUES(?,?,?,?)",
                        params![conversation, &event.data[16..32], required, username],
                    )?;
                    if !username.is_empty() {
                        tx.execute(
                            "INSERT OR IGNORE INTO disclosed_usernames VALUES(?,?,?)",
                            params![conversation, &event.data[16..32], username],
                        )?;
                    }
                }

                super::RESPONSE_EVENT => {
                    tx.execute(
                        "INSERT OR IGNORE INTO invitation_responses
                         SELECT ?,?,? WHERE EXISTS (SELECT 1 FROM invitation_batches WHERE conversation=?)",
                        params![conversation, &event.data[16..32], event.data[32] == 1, conversation],
                    )?;
                }

                _ => {
                    let metadata: Option<String> = tx
                        .query_row(
                            "SELECT data FROM chat_metadata WHERE conversation=?",
                            [conversation],
                            |row| row.get(0),
                        )
                        .optional()?;
                    let mut metadata = metadata
                        .as_deref()
                        .map(serde_json::from_str::<Metadata>)
                        .transpose()?;
                    let mut data = &event.data[16..];
                    while !data.is_empty() {
                        let end = 17 + usize::from(data[16]);
                        let username = std::str::from_utf8(&data[17..end])?;
                        tx.execute(
                            "INSERT OR REPLACE INTO disclosed_usernames VALUES(?,?,?)",
                            params![conversation, &data[..16], username],
                        )?;
                        if let Some(metadata) = &mut metadata
                            && metadata
                                .title_from
                                .as_ref()
                                .is_some_and(|member| *member == data[..16])
                            && let Ok(member) = <[u8; 16]>::try_from(&data[..16])
                            && metadata.title == Id(member).to_string()
                            && !metadata
                                .partners
                                .iter()
                                .any(|(local_member, _)| local_member.as_slice() == &data[..16])
                        {
                            metadata.title = username.to_owned();
                            metadata.title_from = None;
                            metadata.automatic_title = true;
                            tx.execute(
                                "UPDATE chat_metadata SET data=? WHERE conversation=?",
                                params![serde_json::to_string(metadata)?, conversation],
                            )?;
                        }

                        data = &data[end..];
                    }
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles;
    use std::os::unix::fs::DirBuilderExt;

    #[test]
    fn unnamed_chat_titles_follow_disclosed_membership() -> anyhow::Result<()> {
        let db = Connection::open_in_memory()?;
        db.execute(
            "CREATE TABLE disclosed_usernames(conversation BLOB, member BLOB, username TEXT)",
            [],
        )?;
        let chat = Id([9; 16]);
        let own = Id([1; 16]);
        let members = [
            own,
            Id([2; 16]),
            Id([3; 16]),
            Id([4; 16]),
            Id([5; 16]),
            Id([6; 16]),
        ];
        for (member, name) in [
            (Id([7; 16]), "departed"),
            (members[4], "erik2"),
            (members[3], "alice"),
            (members[2], "luca"),
            (members[1], "jones"),
            (own, "me"),
        ] {
            db.execute(
                "INSERT INTO disclosed_usernames VALUES(?,?,?)",
                params![chat.0, member.0, name],
            )?;
        }

        assert_eq!(
            chat_title(&db, chat, None, &[own, members[4]], Some(own))?,
            "erik2"
        );
        assert_eq!(
            chat_title(&db, chat, None, &members, Some(own))?,
            "jones, luca, alice & 1 more"
        );
        assert_eq!(
            chat_title(&db, chat, None, &[own, members[5]], Some(own))?,
            "Chat · 09090909"
        );
        Ok(())
    }

    #[test]
    fn invitation_policy_and_disclosed_names_survive_reopening_a_profile() -> anyhow::Result<()> {
        let directory = std::env::temp_dir().join(format!("faith-test-{}", Id::random()?));
        let run = || -> anyhow::Result<()> {
            let profile = profiles::prepare_claim(&directory, "Local")?;
            assert!(profiles::discover(&directory)?.is_empty());

            let auth = Id([1; 16]);
            let partner = Id([2; 16]);
            let device = Id([3; 16]);
            let outgoing = Id([4; 16]);
            let incoming = Id([5; 16]);
            let packet = GroupPacket {
                kind: GroupKind::Create,
                revision: 0,
                conversation: outgoing,
                message: Id([6; 16]),
                members: vec![auth, partner],
                text: Vec::new(),
            };

            let store = Store::open(&profile.path)?;
            store.enqueue(
                &packet,
                &packet.members,
                Some(&Metadata {
                    title: partner.to_string(),
                    title_from: Some(partner.0),
                    automatic_title: false,
                    partners: Vec::new(),
                }),
                Some(InvitationPolicy {
                    required: false,
                    disclose: true,
                }),
            )?;
            let request = [incoming.0.as_slice(), &partner.0].concat();
            let response = [outgoing.0.as_slice(), &partner.0, &[1]].concat();
            let invited = [incoming.0.as_slice(), &partner.0, &[7], b"Faith_1"].concat();
            let accepted = [outgoing.0.as_slice(), &partner.0, &[7], b"Faith_1"].concat();
            for (sequence, kind, data) in [
                (1, super::super::REQUEST_EVENT, request.as_slice()),
                (2, super::super::USERNAMES_EVENT, invited.as_slice()),
                (3, super::super::RESPONSE_EVENT, response.as_slice()),
                (4, super::super::USERNAMES_EVENT, accepted.as_slice()),
            ] {
                store.apply(StoredEvent {
                    auth,
                    device,
                    sequence,
                    kind,
                    data,
                })?;
            }

            drop(store);
            let store = Store::open(&profile.path)?;
            let requests = store.requests()?;
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].conversation, incoming);
            assert!(!requests[0].required);
            assert_eq!(requests[0].username, "Faith_1");

            let invitations = store.invitations(outgoing)?.unwrap();
            assert!(!invitations.policy.required);
            assert!(invitations.policy.disclose);
            assert_eq!(invitations.responses, vec![(partner, true)]);
            assert_eq!(store.pending()?.unwrap().0.conversation, outgoing);
            let metadata: String = store.db()?.query_row(
                "SELECT data FROM chat_metadata WHERE conversation=?",
                [outgoing.0],
                |row| row.get(0),
            )?;
            let metadata: Metadata = serde_json::from_str(&metadata)?;
            assert_eq!(metadata.title, "Faith_1");
            assert!(metadata.title_from.is_none());
            Ok(())
        };

        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let result = run();
        let cleanup = fs::remove_dir_all(&directory);
        result?;
        cleanup?;
        Ok(())
    }
}
