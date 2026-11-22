use ui::animation::Curve;
mod assets;
mod chat_ui;
mod cli;
mod client;
mod contacts;
mod contacts_ui;
mod graphics;
mod platform;
mod profiles;
mod quickshell;
mod settings;
mod system_fonts;
mod ui;

use clap::{CommandFactory, FromArgMatches};
use graphics::{GraphicsDevice, Renderer};
use platform::{Platform, WindowConfig, WindowEvent, event::Key, mouse::CursorPosition};
use profiles::Profile;
use ui::{
    framework::{DivOptions, Font, Fonts, Framework, Style},
    renderer::Image,
    theme::{self, Catalog},
    widgets::input::InputOptions,
};

// Icons are kept separate from the normal text fonts.
const MENU: &str = "\u{eb94}";
const ACCOUNTS: &str = "\u{f0b55}";
const ACCOUNTS_SELECTED: &str = "\u{f0009}";
const GROUP: &str = "\u{f0849}";
const COPY_AUTH: &str = "󰆏";
const REQUESTS: &str = "\u{f01ee}";
const NEW_CHAT: &str = "";
const CHATS: &str = "\u{f0ede}";
const CHATS_SELECTED: &str = "\u{f0b79}";
const SETTINGS: &str = "\u{f08bb}";
const SETTINGS_SELECTED: &str = "\u{f0493}";
const THEME: &str = "\u{eac6}";
const MORE: &str = "\u{ea7c}";
const ADD: &str = "\u{ea60}";
const LOCK: &str = "\u{ea75}";
const BACK: &str = "\u{eab5}";
const NEXT: &str = "\u{eab6}";
const REFRESH: &str = "\u{eb37}";

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Accounts,
    Contacts(Option<faith_client::Id>),
    Create,
    CreateUsername,
    Conversations,
    Requests,
    PendingInvites,
    CreateConversation,
    Settings,
}

impl Page {
    fn tab(self) -> Tab {
        match self {
            Self::Accounts | Self::Create | Self::CreateUsername => Tab::Profiles,
            Self::Conversations
            | Self::Requests
            | Self::PendingInvites
            | Self::CreateConversation => Tab::Conversations,
            Self::Contacts(_) => Tab::Contacts,
            Self::Settings => Tab::Settings,
        }
    }

    fn is_chat(self) -> bool {
        self.tab() == Tab::Conversations
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Profiles,
    Conversations,
    Contacts,
    Settings,
}

impl Tab {
    fn root(self) -> Page {
        match self {
            Self::Profiles => Page::Accounts,
            Self::Conversations => Page::Conversations,
            Self::Contacts => Page::Contacts(None),
            Self::Settings => Page::Settings,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SettingsItem {
    Appearance,
    Misc,
}

/* we bring back the subsection and list position along with the page */
#[derive(Clone, Copy, PartialEq)]
struct Location {
    page: Page,
    settings_item: SettingsItem,
    conversation: Option<faith_client::Id>,
    chat_session: u64,
    creation: Option<(u64, bool)>,
    list_offset: usize,
}

#[derive(Clone, Copy)]
struct TabLocation {
    location: Location,
    contact_mode: contacts_ui::Mode,
}

#[derive(Clone)]
enum RowMenu {
    Profile(Profile),
    Contact(faith_client::Id),
    Conversation {
        profile: Profile,
        id: faith_client::Id,
    },
}

enum PopupMenu {
    Header,
    Row(RowMenu),
    ConfirmDeleteProfile(Profile),
}

#[derive(Clone)]
enum Action {
    None,
    OpenMenu,
    OpenRowMenu(RowMenu, CursorPosition),
    ConfirmDeleteProfile(Profile),
    DeleteProfile(Profile),
    OpenQuickshell,
    OpenContactPicker,
    Contacts,
    OpenContact(faith_client::Id),
    AddContact,
    EditContact(faith_client::Id),
    ConfirmDeleteContact(faith_client::Id),
    ConfirmDeleteContactInList(faith_client::Id),
    CancelContactListDelete,
    CancelContactEdit,
    SaveContact(faith_client::Id, String),
    SaveContactDraft,
    DeleteContact(faith_client::Id),
    ToggleContactPartner(faith_client::Id),
    Quit,
    Select(usize),
    OpenProfile(Profile),
    OpenConversation {
        profile: Profile,
        id: faith_client::Id,
    },
    OpenSettings {
        section: SettingsItem,
        control: Option<&'static str>,
    },
    FocusSetting {
        action: Box<Action>,
        section: SettingsItem,
        control: &'static str,
    },
    ResetZoom,
    Create(String),
    ClaimUsername(bool),
    ToggleUsernameHelp,
    ProfileUsername(String),
    ToggleUsernameAdvanced,
    AutoDiscloseUsername(bool),
    RejectRequiredUsername(bool),
    CreateWithUsername(String),
    Choose,
    Back,
    Forward,
    New,
    Retry,
    Chats,
    Settings,
    ToggleMenu,
    ListPage(usize),
    NewConversation(Profile),
    MessageRequests(Profile),
    PendingInvites(Profile),
    RequireConversationUsername(bool),
    DiscloseConversationUsername(bool),
    RequestDisclosure(faith_client::Id, bool),
    RespondToRequest {
        profile: Profile,
        conversation: faith_client::Id,
        accept: bool,
        disclose: bool,
    },
    AddChatPartner(faith_client::Id, String),
    FocusChatPartner,
    NameNewConversation(Option<(faith_client::Id, String)>),
    BeginConversationRename {
        profile: Profile,
        id: faith_client::Id,
    },
    OpenConversationContacts,
    PromptSaveConversationContact {
        profile: Profile,
        id: faith_client::Id,
        member: faith_client::Id,
    },
    SaveConversationContact {
        profile: Profile,
        id: faith_client::Id,
        member: faith_client::Id,
        name: String,
    },
    CancelConversationRename,
    RenameConversation {
        profile: Profile,
        id: faith_client::Id,
    },
    RemoveChatPartner(usize),
    CancelConversation,
    CreateConversation {
        title: String,
        partners: Vec<(faith_client::Id, String)>,
        policy: client::InvitationPolicy,
        automatic_title: bool,
    },
    SendMessage {
        id: faith_client::Id,
        body: String,
    },
    CopyAuthId(Profile),
    RetryDelivery(Profile),
    DiscardDelivery(Profile),
    Theme(String),
    OpenFontPicker,
    DiscoverFonts,
    Font(Option<String>),
    SettingsItem(SettingsItem),
    Rounded(bool),
    Animations(bool),
    LayoutMorphing(bool),
    ToggleMorphAdvanced,
    MorphStrength(u32),
    MorphDuration(u32),
    MorphCurve(Curve),
    AnimationCurve(Curve),
    SmoothScrolling(bool),
    ZoomStep(u32),
    DefaultZoom(u32),
}

struct ProfileForm {
    claim: bool,
    help: bool,
    advanced: bool,
    alias: String,
    username: profiles::UsernameSettings,
    pending: Option<(Profile, client::Session)>,
}

impl Default for ProfileForm {
    fn default() -> Self {
        Self {
            claim: true,
            help: false,
            advanced: false,
            alias: String::new(),
            username: profiles::UsernameSettings::default(),
            pending: None,
        }
    }
}

// State shared by the pages.
struct App {
    root: std::path::PathBuf,
    contacts: contacts::Book,
    contact_mode: contacts_ui::Mode,
    contact_delete_in_list: Option<faith_client::Id>,
    contact_usernames: std::collections::HashMap<faith_client::Id, String>,
    contact_conversations: Vec<(Profile, Vec<client::Chat>)>,
    contact_links: Vec<contacts_ui::ConversationLink>,
    profiles: Vec<Profile>,
    profile_form: ProfileForm,
    selected: Option<Profile>,
    session: Option<client::Session>,
    chat: chat_ui::State,
    page: Page,
    tab_locations: [Option<TabLocation>; 4],
    back_history: Vec<Location>,
    forward_history: Vec<Location>,
    error: String,
    collapsed: bool,
    list_offset: usize,
    conversation: Option<faith_client::Id>,
    theme: String,
    themes: Catalog,
    font: Option<system_fonts::Family>,
    system_fonts: system_fonts::Catalog,
    settings_item: SettingsItem,
    rounded: bool,
    animations: bool,
    layout_morphing: bool,
    morph_duration_ms: u32,
    morph_curve: Curve,
    morph_strength: u32,
    morph_advanced: bool,
    animation_curve: Curve,
    smooth_scrolling: bool,
    zoom_step: u32,
    default_zoom: u32,
    settings: settings::Store,
    saved_at: Option<std::time::Instant>,
    copied_at: Option<std::time::Instant>,
    settings_error: String,
}

// Account sessions and actions returned by the pages.
impl App {
    fn location(&self) -> Location {
        Location {
            page: self.page,
            settings_item: self.settings_item,
            conversation: self.conversation,
            chat_session: self.chat.session,
            creation: (self.page == Page::CreateConversation).then(|| self.chat.creation_step()),
            list_offset: self.list_offset,
        }
    }

    fn switch_tab(&mut self, tab: Tab) {
        if self.page.tab() == tab {
            return;
        }

        let saved = self.tab_locations[tab as usize].filter(|saved| {
            tab != Tab::Conversations || saved.location.chat_session == self.chat.session
        });
        let location = saved.map_or_else(
            || Location {
                page: tab.root(),
                conversation: None,
                creation: None,
                list_offset: 0,
                ..self.location()
            },
            |saved| saved.location,
        );
        self.restore_location(location);
        if let Some(saved) = saved
            && tab == Tab::Contacts
            && self.page == saved.location.page
        {
            self.contact_mode = saved.contact_mode;
        }
    }

    fn restore_location(&mut self, location: Location) {
        self.error.clear();
        self.page = location.page;
        if self.page == Page::PendingInvites && self.chat.snapshot.pending_invites.is_empty() {
            self.page = Page::Conversations;
        }

        if self.page == Page::CreateConversation {
            match location.creation {
                Some((form, naming))
                    if location.chat_session == self.chat.session
                        && form == self.chat.creation_step().0 =>
                {
                    self.chat.naming = naming && !self.chat.partners.is_empty();
                }
                _ => self.page = Page::Conversations,
            }
        }

        if let Page::Contacts(id) = self.page {
            self.page = Page::Contacts(id.filter(|&id| self.contacts.get(id).is_some()));
            self.refresh_contact_conversations();
            self.refresh_contact_links();
        }

        self.contact_mode = contacts_ui::Mode::View;
        if self.page == Page::Settings {
            self.settings_item = location.settings_item;
        }

        self.conversation = location.conversation.filter(|id| {
            location.chat_session == self.chat.session
                && self
                    .chat
                    .snapshot
                    .conversations
                    .iter()
                    .any(|chat| chat.id == *id)
        });

        if self.page == Page::Conversations
            && let Some(id) = self.conversation
            && let Some(session) = &self.session
            && let Err(error) = session.submit(client::Command::Select(id))
        {
            self.error = error.to_string();
        }

        self.list_offset = location.list_offset;
    }

    fn clear_conversation_form(&mut self, ui: &mut Framework) {
        self.chat.clear_form(ui);
        if self.tab_locations[Tab::Conversations as usize]
            .is_some_and(|saved| saved.location.page == Page::CreateConversation)
        {
            self.tab_locations[Tab::Conversations as usize] = None;
        }

        self.back_history
            .retain(|location| location.page != Page::CreateConversation);
        self.forward_history
            .retain(|location| location.page != Page::CreateConversation);
    }

    fn save_settings(&mut self) {
        self.saved_at = None;
        self.settings_error.clear();
        self.settings.save(settings::Settings {
            theme: self.theme.clone(),
            font: self.font.clone(),
            rounded_corners: self.rounded,
            animations: self.animations,
            layout_morphing: self.layout_morphing,
            morph_duration_ms: self.morph_duration_ms,
            morph_curve: self.morph_curve,
            morph_strength: self.morph_strength,
            animation_curve: self.animation_curve,
            smooth_scrolling: self.smooth_scrolling,
            zoom_step: self.zoom_step,
            default_zoom: self.default_zoom,
        });
    }

    fn login(&mut self, profile: Profile, cli: &cli::Cli, platform: &Platform) {
        // Stop the old session before selecting another account.
        self.session = None;
        self.chat.reset();
        self.error.clear();
        self.conversation = None;
        self.list_offset = 0;
        self.page = Page::Conversations;
        self.selected = Some(profile.clone());

        match Self::start_profile(&profile, None, cli, platform) {
            Ok(session) => self.session = Some(session),
            Err(error) => self.error = error.to_string(),
        }
    }

    fn delete_profile(
        &mut self,
        profile: &Profile,
        cli: &cli::Cli,
        platform: &Platform,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.profiles.iter().any(|item| item.path == profile.path),
            "Profile is unavailable"
        );
        let active = self.selected.as_ref().is_some_and(|item| {
            item.path == profile.path
                || item
                    .path
                    .canonicalize()
                    .ok()
                    .zip(profile.path.canonicalize().ok())
                    .is_some_and(|(active, target)| active == target)
        });
        if active {
            self.session = None;
        }

        let session_path = self
            .selected
            .as_ref()
            .filter(|_| active)
            .map_or(profile.path.as_path(), |item| item.path.as_path());
        let result = client::Session::wait_for_close(session_path)
            .and_then(|()| profiles::delete(&self.root, profile));
        let cleanup_error = match result {
            Ok(cleanup_error) => cleanup_error,
            Err(error) => {
                if active {
                    self.chat.reset();
                    self.session = self.selected.as_ref().and_then(|selected| {
                        Self::start_profile(selected, None, cli, platform).ok()
                    });
                }

                return Err(error);
            }
        };

        self.profiles.retain(|item| item.path != profile.path);
        self.contact_conversations
            .retain(|(item, _)| item.path != profile.path);
        if active {
            self.selected = None;
            self.chat.reset();
            self.conversation = None;
            self.back_history
                .retain(|location| !location.page.is_chat());
            self.forward_history
                .retain(|location| !location.page.is_chat());
            if self.page.is_chat() {
                self.page = Page::Accounts;
            }
        }

        self.load_contact_usernames(None);
        self.error = cleanup_error.map_or_else(String::new, |error| {
            format!("Profile was removed, but its files could not all be erased: {error}")
        });
        Ok(())
    }

    fn start_profile(
        profile: &Profile,
        claim: Option<profiles::UsernameSettings>,
        cli: &cli::Cli,
        platform: &Platform,
    ) -> anyhow::Result<client::Session> {
        client::Session::start(
            faith_client::Config {
                host: cli.host.clone(),
                port: cli.port,
                server_name: cli.server_name.clone(),
                ca_file: cli.ca_file.clone(),
                identity_path: profile.path.join("identity.bin"),
                store: None,
            },
            claim,
            platform.waker(),
        )
    }

    fn ensure_profile(
        &mut self,
        profile: Profile,
        cli: &cli::Cli,
        platform: &Platform,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(profile.path.is_dir(), "Profile is no longer available");
        if self
            .selected
            .as_ref()
            .is_none_or(|selected| selected.path != profile.path)
        {
            self.login(profile, cli, platform);
        }

        Ok(())
    }

    fn submit_chat(&self, command: client::Command) -> anyhow::Result<u64> {
        self.session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Profile session is unavailable"))?
            .submit(command)
    }

    fn load_contact_usernames(&mut self, extra_profile: Option<&std::path::Path>) {
        self.contact_usernames.clear();
        let extra_profile =
            extra_profile.or_else(|| self.selected.as_ref().map(|profile| profile.path.as_path()));
        for path in self
            .profiles
            .iter()
            .map(|profile| profile.path.as_path())
            .chain(extra_profile)
        {
            match client::profile_usernames(path) {
                Ok(usernames) => self.contact_usernames.extend(
                    usernames
                        .into_iter()
                        .filter(|(_, username)| !username.is_empty()),
                ),
                Err(error) => tracing::warn!(
                    "Could not read disclosed names from {}: {error}",
                    path.display()
                ),
            }
        }

        self.sync_contact_names();
    }

    fn sync_contact_names(&mut self) {
        self.contacts.apply_usernames(&self.contact_usernames);
        self.chat.invalidate_history_layout();
        self.refresh_contact_links();
    }

    fn refresh_contact_conversations(&mut self) {
        self.contact_conversations.clear();
        for profile in self
            .profiles
            .iter()
            .chain(self.selected.iter().filter(|active| {
                !self
                    .profiles
                    .iter()
                    .any(|profile| profile.path == active.path)
            }))
        {
            if self
                .selected
                .as_ref()
                .is_some_and(|active| active.path == profile.path)
                && self.chat.snapshot.auth.is_some()
            {
                continue;
            }

            match client::profile_conversations(&profile.path) {
                Ok(chats) => self.contact_conversations.push((profile.clone(), chats)),
                Err(error) => tracing::warn!(
                    "Could not list conversations for {}: {error}",
                    profile.alias
                ),
            }
        }
    }

    fn refresh_contact_links(&mut self) {
        let app = &*self;
        self.contact_links = if let Page::Contacts(Some(id)) = self.page {
            let active = self
                .selected
                .iter()
                .filter(|_| self.chat.snapshot.auth.is_some())
                .map(|profile| (profile, self.chat.snapshot.conversations.as_ref()));
            self.contact_conversations
                .iter()
                .filter(|(profile, _)| {
                    self.chat.snapshot.auth.is_none()
                        || self
                            .selected
                            .as_ref()
                            .is_none_or(|active| active.path != profile.path)
                })
                .map(|(profile, chats)| (profile, chats.as_slice()))
                .chain(active)
                .flat_map(|(profile, chats)| {
                    chats
                        .iter()
                        .filter(move |chat| chat.members.contains(&id))
                        .map(move |chat| contacts_ui::ConversationLink {
                            key: format!("contact-chat:{}:{}", profile.path.display(), chat.id),
                            profile: profile.clone(),
                            id: chat.id,
                            title: app.conversation_title(chat).into_owned(),
                            detail: format!("On profile {}", profile.alias),
                        })
                })
                .collect()
        } else {
            Vec::new()
        };
    }

    fn contact_name<'a>(&'a self, id: faith_client::Id, fallback: &'a str) -> &'a str {
        self.contacts.display(id, fallback)
    }

    fn can_save_partner(&self, member: faith_client::Id) -> bool {
        self.chat.snapshot.auth.is_some_and(|auth| auth != member)
            && self.contacts.get(member).is_none()
    }

    fn check_conversation_contact(
        &self,
        profile: &Profile,
        id: faith_client::Id,
        member: faith_client::Id,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.selected
                .as_ref()
                .is_some_and(|active| active.path == profile.path)
                && self.can_save_partner(member)
                && self
                    .chat
                    .snapshot
                    .conversations
                    .iter()
                    .any(|chat| { chat.id == id && chat.members.contains(&member) }),
            "Conversation partner is unavailable or already in Contacts"
        );
        Ok(())
    }

    fn partner_label(&self, id: &faith_client::Id, draft_name: &str) -> String {
        if !draft_name.is_empty() {
            return contacts::display_name(
                draft_name,
                self.contact_usernames.get(id).map(String::as_str),
                "",
            );
        }

        let name = self.contact_name(
            *id,
            self.contact_usernames.get(id).map_or("", String::as_str),
        );
        chat_ui::partner_label(id, name)
    }

    fn default_conversation_title(&self) -> String {
        self.chat
            .default_title(|id, name| self.partner_label(id, name))
    }

    fn conversation_title<'a>(&'a self, chat: &'a client::Chat) -> std::borrow::Cow<'a, str> {
        if !chat.name_members {
            return (&chat.title).into();
        }

        self.member_title(&chat.members, chat.own_auth, &chat.title)
    }

    fn member_title<'a>(
        &'a self,
        members: &[faith_client::Id],
        own_auth: Option<faith_client::Id>,
        fallback: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        let mut peers = members
            .iter()
            .copied()
            .filter(|&id| Some(id) != own_auth)
            .filter_map(|id| {
                let name = self.contact_name(
                    id,
                    self.contact_usernames.get(&id).map_or("", String::as_str),
                );
                (!name.is_empty()).then_some(name)
            });
        let Some(first) = peers.next() else {
            return fallback.into();
        };

        let Some(second) = peers.next() else {
            return first.into();
        };

        let mut title = format!("{first}, {second}");
        if let Some(third) = peers.next() {
            title.push_str(", ");
            title.push_str(third);
        }

        let remaining = peers.count();
        if remaining != 0 {
            title.push_str(&format!(" & {remaining} more"));
        }

        title.into()
    }

    fn refresh_chat(&mut self, ui: &mut Framework) {
        if let Some((_, session)) = &self.profile_form.pending
            && let Some(snapshot) = session.snapshot_since(0)
        {
            if snapshot.claim_completed {
                if self.tab_locations[Tab::Profiles as usize]
                    .is_some_and(|saved| saved.location.page == Page::CreateUsername)
                {
                    self.tab_locations[Tab::Profiles as usize] = None;
                }

                let (profile, session) = self.profile_form.pending.take().unwrap();
                self.profiles
                    .retain(|existing| existing.path != profile.path);
                self.profiles.push(profile.clone());
                self.profiles.sort_by(|a, b| a.alias.cmp(&b.alias));
                self.back_history
                    .retain(|location| location.page != Page::CreateUsername);
                self.forward_history
                    .retain(|location| location.page != Page::CreateUsername);

                if self.page == Page::CreateUsername {
                    self.session = Some(session);
                    self.selected = Some(profile);
                    self.chat.reset();
                    self.error.clear();
                    self.conversation = None;
                    self.list_offset = 0;
                    self.page = Page::Conversations;
                    ui.navigation_changed();
                }
            } else if let Some(error) = snapshot.auth_error {
                self.profile_form.pending = None;

                if self.page == Page::CreateUsername {
                    self.error = error;
                }
            }
        }

        let Some(snapshot) = self
            .session
            .as_ref()
            .and_then(|session| session.snapshot_since(self.chat.snapshot.generation))
        else {
            return;
        };

        if let Some((id, accepted, _created)) = snapshot.submission
            && self
                .chat
                .submission
                .as_ref()
                .is_some_and(|(request, _)| *request == id)
            && let Some((_, submission)) = self.chat.submission.take()
        {
            if accepted {
                match submission {
                    chat_ui::Submission::Create => {
                        self.clear_conversation_form(ui);
                        if self.page == Page::CreateConversation {
                            self.conversation = None;
                            self.page = Page::Conversations;
                            ui.navigation_changed();
                        }
                    }

                    chat_ui::Submission::Send(input) => {
                        ui.clear_input(&input);
                        if self.page == Page::Conversations
                            && self
                                .conversation
                                .is_some_and(|id| self.chat.composer_id(id) == input)
                        {
                            ui.focus_input(&input);
                        }
                    }
                }
            } else {
                self.error = snapshot
                    .notices
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "Chat action failed".into());
            }
        }

        self.chat.request_disclosure.retain(|id, _| {
            snapshot
                .requests
                .iter()
                .any(|request| request.conversation == *id)
        });
        if !std::sync::Arc::ptr_eq(&self.chat.snapshot.disclosed, &snapshot.disclosed) {
            let mut changed = false;
            for (id, username) in snapshot
                .disclosed
                .iter()
                .filter(|(_, username)| !username.is_empty())
            {
                if self.contact_usernames.get(id) != Some(username) {
                    self.contact_usernames.insert(*id, username.clone());
                    changed = true;
                }
            }

            if changed {
                self.sync_contact_names();
            }
        }

        self.chat.snapshot = snapshot;
        if matches!(self.page, Page::Contacts(Some(_))) {
            self.refresh_contact_links();
        }
        if self.page == Page::PendingInvites && self.chat.snapshot.pending_invites.is_empty() {
            self.page = Page::Conversations;
            ui.navigation_changed();
        }
    }

    fn act(
        &mut self,
        action: Action,
        cli: &cli::Cli,
        platform: &Platform,
        ui: &mut Framework,
        clipboard: &mut Vec<ui::framework::ClipboardAction>,
    ) -> anyhow::Result<()> {
        if let Action::FocusSetting {
            action,
            section,
            control,
        } = action
        {
            self.act(*action, cli, platform, ui, clipboard)?;
            return self.act(
                Action::OpenSettings {
                    section,
                    control: Some(control),
                },
                cli,
                platform,
                ui,
                clipboard,
            );
        }

        let action = if matches!(action, Action::SaveContactDraft) {
            let id = match (self.contact_mode, self.page) {
                (contacts_ui::Mode::Add, Page::Contacts(None)) => {
                    let id = ui
                        .input_value("contact-auth")
                        .unwrap_or_default()
                        .parse::<faith_client::Id>()?;
                    anyhow::ensure!(
                        self.contacts.get(id).is_none(),
                        "This Auth ID already has a contact"
                    );
                    id
                }
                (contacts_ui::Mode::Edit, Page::Contacts(Some(id))) => id,
                _ => return Ok(()),
            };
            Action::SaveContact(
                id,
                ui.input_value("contact-name")
                    .unwrap_or_default()
                    .to_owned(),
            )
        } else {
            action
        };

        // Apply the action after the current frame has finished.
        let previous = self.location();
        let previous_contact_mode = self.contact_mode;
        let creating_profile = matches!(action, Action::Create(_));
        let begin_rename = matches!(action, Action::BeginConversationRename { .. });
        let focus_requested = matches!(
            action,
            Action::New
                | Action::AddContact
                | Action::EditContact(_)
                | Action::BeginConversationRename { .. }
                | Action::ProfileUsername(_)
                | Action::NewConversation(_)
                | Action::NameNewConversation(_)
                | Action::OpenSettings {
                    control: Some(_),
                    ..
                }
        );
        let record_history = matches!(
            action,
            Action::Select(_)
                | Action::AddContact
                | Action::Contacts
                | Action::OpenContact(_)
                | Action::ConfirmDeleteContact(_)
                | Action::Create(_)
                | Action::ProfileUsername(_)
                | Action::OpenProfile(_)
                | Action::OpenConversation { .. }
                | Action::BeginConversationRename { .. }
                | Action::MessageRequests(_)
                | Action::PendingInvites(_)
                | Action::NewConversation(_)
                | Action::NameNewConversation(_)
                | Action::OpenSettings { .. }
                | Action::Choose
                | Action::New
                | Action::Chats
                | Action::Settings
                | Action::SettingsItem(_)
        );
        match action {
            Action::FocusSetting { .. } | Action::SaveContactDraft => unreachable!(),
            Action::None
            | Action::OpenMenu
            | Action::OpenRowMenu(..)
            | Action::ConfirmDeleteProfile(_)
            | Action::OpenQuickshell
            | Action::OpenFontPicker
            | Action::OpenContactPicker
            | Action::Quit
            | Action::OpenConversationContacts
            | Action::PromptSaveConversationContact { .. } => {
                return Ok(());
            }
            Action::Contacts => self.switch_tab(Tab::Contacts),

            Action::OpenContact(id) => {
                anyhow::ensure!(self.contacts.get(id).is_some(), "Contact is unavailable");
                if !matches!(self.page, Page::Contacts(_)) {
                    self.refresh_contact_conversations();
                    self.list_offset = 0;
                }

                self.page = Page::Contacts(Some(id));
                self.refresh_contact_links();
                self.contact_mode = contacts_ui::Mode::View;
                self.error.clear();
            }

            Action::AddContact => {
                self.contact_delete_in_list = None;
                if !matches!(self.page, Page::Contacts(_)) {
                    self.refresh_contact_conversations();
                }

                self.page = Page::Contacts(None);
                self.contact_mode = contacts_ui::Mode::Add;
                self.error.clear();
                ui.clear_input("contact-auth");
                ui.clear_input("contact-name");
                ui.focus_input("contact-auth");
            }

            Action::EditContact(id) => {
                self.contact_delete_in_list = None;
                let contact = self
                    .contacts
                    .get(id)
                    .ok_or_else(|| anyhow::anyhow!("Contact is unavailable"))?;
                self.page = Page::Contacts(Some(id));
                self.contact_mode = contacts_ui::Mode::Edit;
                self.error.clear();
                ui.set_input("contact-name", &contact.name);
                ui.focus_input("contact-name");
            }

            Action::ConfirmDeleteContact(id) => {
                self.contact_delete_in_list = None;
                anyhow::ensure!(self.contacts.get(id).is_some(), "Contact is unavailable");
                if !matches!(self.page, Page::Contacts(_)) {
                    self.refresh_contact_conversations();
                }

                self.page = Page::Contacts(Some(id));
                self.refresh_contact_links();
                self.contact_mode = contacts_ui::Mode::Delete;
                self.error.clear();
            }

            Action::ConfirmDeleteContactInList(id) => {
                anyhow::ensure!(
                    matches!(self.page, Page::Contacts(_)) && self.contacts.get(id).is_some(),
                    "Contact is unavailable"
                );
                self.contact_delete_in_list = Some(id);
                self.error.clear();
            }

            Action::CancelContactListDelete => {
                self.contact_delete_in_list = None;
                self.error.clear();
            }

            Action::CancelContactEdit => {
                ui.clear_input("contact-name");
                self.contact_mode = contacts_ui::Mode::View;
                self.error.clear();
            }

            Action::SaveContact(id, name) => {
                if self.contact_mode == contacts_ui::Mode::Edit && name.trim().is_empty() {
                    self.contacts.remove_nickname(id)?;
                } else {
                    self.contacts.save(&[(id, name)])?;
                }

                self.page = Page::Contacts(Some(id));
                self.sync_contact_names();
                ui.clear_input("contact-name");
                self.contact_mode = contacts_ui::Mode::View;
                self.error.clear();
            }

            Action::DeleteContact(id) => {
                if let Page::Contacts(selected) = self.page
                    && (self.contact_delete_in_list == Some(id)
                        || selected == Some(id) && self.contact_mode == contacts_ui::Mode::Delete)
                {
                    self.contacts.delete(id)?;
                    self.contact_delete_in_list = None;
                    if selected == Some(id) {
                        self.page = Page::Contacts(None);
                        self.contact_mode = contacts_ui::Mode::View;
                        ui.clear_input("contact-name");
                    }

                    self.sync_contact_names();
                    self.error.clear();
                }
            }

            Action::ToggleContactPartner(id) => {
                if self.page == Page::CreateConversation
                    && self.chat.editing_creation()
                    && !self.chat.naming
                    && self.contacts.get(id).is_some()
                {
                    self.chat.toggle_contact(id);
                }
            }

            Action::Back => {
                let destination = self.back_history.pop().or_else(|| {
                    (self.page != Page::Accounts).then_some(Location {
                        page: Page::Accounts,
                        ..previous
                    })
                });
                if let Some(destination) = destination {
                    self.forward_history.push(previous);
                    self.restore_location(destination);
                }
            }
            Action::Forward => {
                if let Some(destination) = self.forward_history.pop() {
                    self.back_history.push(previous);
                    self.restore_location(destination);
                }
            }
            Action::DeleteProfile(profile) => {
                self.delete_profile(&profile, cli, platform)?;
                ui.navigation_changed();
            }

            Action::Select(index) => self.login(self.profiles[index].clone(), cli, platform),
            Action::OpenProfile(profile) => {
                anyhow::ensure!(profile.path.is_dir(), "Profile is no longer available");
                if self
                    .selected
                    .as_ref()
                    .is_none_or(|selected| selected.path != profile.path)
                {
                    self.login(profile, cli, platform);
                } else {
                    self.page = Page::Conversations;
                }
            }
            Action::OpenConversation { profile, id } => {
                self.ensure_profile(profile, cli, platform)?;
                self.submit_chat(client::Command::Select(id))?;
                self.conversation = Some(id);
                self.page = Page::Conversations;
                self.error.clear();
            }

            Action::MessageRequests(profile) => {
                self.ensure_profile(profile, cli, platform)?;
                self.page = Page::Requests;
                self.error.clear();
            }

            Action::PendingInvites(profile) => {
                self.ensure_profile(profile, cli, platform)?;
                self.page = Page::PendingInvites;
                self.error.clear();
            }

            Action::RequireConversationUsername(value) => {
                self.chat.require_username = value;
            }

            Action::DiscloseConversationUsername(value) => {
                self.chat.disclose_username = Some(value);
            }

            Action::RequestDisclosure(id, value) => {
                self.chat.request_disclosure.insert(id, value);
            }

            Action::RespondToRequest {
                profile,
                conversation,
                accept,
                disclose,
            } => {
                self.ensure_profile(profile, cli, platform)?;
                self.submit_chat(client::Command::Respond {
                    conversation,
                    accept,
                    disclose,
                })?;
                self.page = Page::Requests;
                self.error.clear();
            }

            Action::NewConversation(profile) => {
                self.ensure_profile(profile, cli, platform)?;
                if self.page != Page::CreateConversation {
                    self.clear_conversation_form(ui);
                    self.page = Page::CreateConversation;
                }

                self.error.clear();
                if self.chat.naming {
                    ui.focus_input(&self.chat.title_id());
                } else {
                    ui.focus_input(&self.chat.partner_id());
                }
            }

            Action::AddChatPartner(_, _)
            | Action::FocusChatPartner
            | Action::RemoveChatPartner(_)
            | Action::NameNewConversation(_)
            | Action::CancelConversation
                if self.page != Page::CreateConversation || !self.chat.editing_creation() => {}

            Action::FocusChatPartner => ui.focus_input(&self.chat.partner_id()),
            Action::AddChatPartner(id, name) => self.chat.add_partner(ui, (id, name)),
            Action::RemoveChatPartner(index) => {
                if index < self.chat.partners.len() {
                    self.chat.partners.remove(index);
                }
            }

            Action::NameNewConversation(partner) => {
                if let Some(partner) = partner {
                    self.chat.add_partner(ui, partner);
                }

                self.chat.naming = true;
                ui.set_input(&self.chat.title_id(), &self.default_conversation_title());
            }

            Action::BeginConversationRename { profile, id } => {
                anyhow::ensure!(
                    self.selected
                        .as_ref()
                        .is_some_and(|active| active.path == profile.path),
                    "Conversation profile is unavailable"
                );
                let chat = self
                    .chat
                    .snapshot
                    .conversations
                    .iter()
                    .find(|chat| chat.id == id)
                    .ok_or_else(|| anyhow::anyhow!("Conversation is unavailable"))?;
                let title: String = self
                    .conversation_title(chat)
                    .chars()
                    .take(client::CONVERSATION_NAME_MAX)
                    .collect();
                if !self.page.is_chat() {
                    self.page = Page::Conversations;
                    self.conversation = None;
                    self.list_offset = 0;
                }

                self.chat.renaming = Some(id);
                self.error.clear();
                ui.clear_input(chat_ui::RENAME_INPUT);
                ui.set_input(chat_ui::RENAME_INPUT, &title);
            }

            Action::CancelConversationRename => {
                self.chat.renaming = None;
                ui.clear_input(chat_ui::RENAME_INPUT);
            }

            Action::RenameConversation { profile, id } => {
                anyhow::ensure!(
                    self.page.is_chat()
                        && self.chat.renaming == Some(id)
                        && self
                            .selected
                            .as_ref()
                            .is_some_and(|active| active.path == profile.path),
                    "Conversation is unavailable"
                );
                let title = ui
                    .input_value(chat_ui::RENAME_INPUT)
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                anyhow::ensure!(!title.is_empty(), "Enter a conversation name");

                self.submit_chat(client::Command::Rename { id, title })?;
                self.chat.renaming = None;
                self.error.clear();
                ui.clear_input(chat_ui::RENAME_INPUT);
            }

            Action::SaveConversationContact {
                profile,
                id,
                member,
                name,
            } => {
                self.check_conversation_contact(&profile, id, member)?;
                self.contacts.save(&[(member, name)])?;
                self.sync_contact_names();
                self.error.clear();
            }

            Action::CancelConversation => {
                self.clear_conversation_form(ui);
                self.page = Page::Conversations;
                self.error.clear();
            }

            Action::CreateConversation {
                title,
                partners,
                policy,
                automatic_title,
            } => {
                let aliases: Vec<_> = partners
                    .iter()
                    .filter(|(_, name)| !name.is_empty())
                    .cloned()
                    .collect();
                if !aliases.is_empty() {
                    self.contacts.save(&aliases)?;
                    self.sync_contact_names();
                }

                let id = self.submit_chat(client::Command::Create {
                    title,
                    partners: partners.into_iter().map(|(id, _)| id).collect(),
                    policy,
                    automatic_title,
                })?;
                self.chat.submission = Some((id, chat_ui::Submission::Create));
                self.error.clear();
            }

            Action::SendMessage { id, body } => {
                let input = self.chat.composer_id(id);
                let id = self.submit_chat(client::Command::Send { id, body })?;
                self.chat.submission = Some((id, chat_ui::Submission::Send(input)));
                self.error.clear();
            }

            Action::CopyAuthId(profile) => {
                self.ensure_profile(profile, cli, platform)?;
                let auth = self
                    .chat
                    .snapshot
                    .auth
                    .ok_or_else(|| anyhow::anyhow!("Auth ID is still loading"))?;
                clipboard.push(ui::framework::ClipboardAction::Copy(auth.to_string()));
            }

            Action::RetryDelivery(profile) => {
                self.ensure_profile(profile, cli, platform)?;
                self.submit_chat(client::Command::Retry)?;
            }

            Action::DiscardDelivery(profile) => {
                self.ensure_profile(profile, cli, platform)?;
                self.submit_chat(client::Command::Discard)?;
            }

            Action::OpenSettings { section, control } => {
                self.page = Page::Settings;
                self.settings_item = section;
                if section == SettingsItem::Appearance
                    && let Err(error) = self.themes.refresh()
                {
                    self.error = error.to_string();
                }

                if let Some(id) = control {
                    ui.focus_control(id, true);
                }
            }
            Action::ResetZoom => ui.reset_zoom(),
            Action::ClaimUsername(claim) => self.profile_form.claim = claim,
            Action::ToggleUsernameHelp => self.profile_form.help = !self.profile_form.help,
            Action::ToggleUsernameAdvanced => {
                self.profile_form.advanced = !self.profile_form.advanced;
            }

            Action::AutoDiscloseUsername(value) => {
                self.profile_form.advanced = true;
                self.profile_form.username.auto_disclose = value;
                ui.focus_control("auto-disclose-username", true);
            }

            Action::RejectRequiredUsername(value) => {
                self.profile_form.advanced = true;
                self.profile_form.username.reject_required = value;
                ui.focus_control("reject-required-username", true);
            }

            Action::ProfileUsername(alias) => {
                profiles::check_claim_alias(&self.root, &alias)?;

                if self.profile_form.alias != alias {
                    ui.set_input("new-profile-username", &alias);
                    self.profile_form.alias = alias;
                }

                self.error.clear();
                self.page = Page::CreateUsername;
                ui.focus_control("new-profile-username", true);
            }

            Action::CreateWithUsername(username) => {
                anyhow::ensure!(
                    profiles::valid_username(&username),
                    profiles::USERNAME_ERROR
                );

                if self.profile_form.pending.is_none() {
                    let profile = profiles::prepare_claim(&self.root, &self.profile_form.alias)?;
                    let mut settings = self.profile_form.username.clone();
                    settings.username = Some(username);
                    let session = Self::start_profile(&profile, Some(settings), cli, platform)?;
                    self.profile_form.pending = Some((profile, session));
                    self.error.clear();
                }
            }

            Action::Create(alias) => match profiles::create(&self.root, &alias) {
                Ok(profile) => {
                    self.profiles.push(profile.clone());
                    self.profiles.sort_by(|a, b| a.alias.cmp(&b.alias));
                    self.login(profile, cli, platform);
                }

                Err(error) => self.error = error.to_string(),
            },

            // Refresh the profiles when opening the account list.
            Action::Choose => {
                self.switch_tab(Tab::Profiles);
                self.error.clear();

                match profiles::discover(&self.root) {
                    Ok(mut profiles) => {
                        if let Some(selected) = &self.selected
                            && !profiles.iter().any(|p| p.path == selected.path)
                        {
                            profiles.push(selected.clone());
                        }

                        self.profiles = profiles;
                    }

                    Err(error) => self.error = error.to_string(),
                }
            }

            // Start with an empty handle when opening the creation form.
            Action::New => {
                self.error.clear();
                if self.profile_form.pending.is_some() {
                    self.page = Page::CreateUsername;
                    return platform.request_redraw();
                }

                self.profile_form = ProfileForm::default();
                ui.clear_input("new-profile-username");
                ui.clear_input("new-profile-handle");
                ui.focus_control("new-profile-handle", true);
                self.page = Page::Create;
            }

            Action::Retry => {
                if let Some(profile) = self.selected.clone() {
                    self.login(profile, cli, platform);
                }
            }

            Action::Chats => self.switch_tab(Tab::Conversations),

            Action::Settings => {
                self.switch_tab(Tab::Settings);
                if let Err(error) = self.themes.refresh() {
                    self.error = format!("Could not discover themes: {error:#}");
                }
            }

            Action::ToggleMenu => self.collapsed = !self.collapsed,
            Action::ListPage(offset) => self.list_offset = offset,
            Action::SettingsItem(item) => {
                if item == SettingsItem::Appearance
                    && let Err(error) = self.themes.refresh()
                {
                    self.error = format!("Could not discover themes: {error:#}");
                }

                self.settings_item = item;
            }

            Action::Rounded(enabled) => {
                self.rounded = enabled;
                ui.set_rounded_corners(enabled);
                self.save_settings();
            }

            Action::Animations(enabled) => {
                self.animations = enabled;
                ui.set_animations(enabled, self.animation_curve);
                self.save_settings();
            }

            Action::LayoutMorphing(enabled) => {
                self.layout_morphing = enabled;
                ui.set_layout_morphing(enabled);
                self.save_settings();
            }

            Action::ToggleMorphAdvanced => self.morph_advanced = !self.morph_advanced,

            Action::MorphStrength(strength) => {
                self.morph_strength = strength.min(100);
                ui.set_morph_parameters(
                    self.morph_duration_ms,
                    self.morph_curve,
                    self.morph_strength,
                );
                self.save_settings();
            }

            Action::MorphDuration(duration) => {
                self.morph_duration_ms = duration.clamp(20, 1000);
                ui.set_morph_parameters(
                    self.morph_duration_ms,
                    self.morph_curve,
                    self.morph_strength,
                );
                self.save_settings();
            }

            Action::MorphCurve(curve) => {
                self.morph_curve = curve;
                ui.set_morph_parameters(
                    self.morph_duration_ms,
                    self.morph_curve,
                    self.morph_strength,
                );
                self.save_settings();
            }

            Action::ZoomStep(step) => {
                self.zoom_step = step.clamp(1, 50);
                ui.set_zoom_step(self.zoom_step);
                self.save_settings();
            }

            Action::DefaultZoom(percent) => {
                self.default_zoom = percent;
                self.save_settings();
            }

            Action::SmoothScrolling(enabled) => {
                self.smooth_scrolling = enabled;
                ui.set_smooth_scrolling(enabled);
                self.save_settings();
            }

            Action::AnimationCurve(curve) => {
                self.animation_curve = curve;
                ui.set_animations(self.animations, curve);
                self.save_settings();
            }

            Action::Theme(theme) => match self.themes.load(&theme) {
                Ok(colors) => {
                    self.theme = theme;
                    self.error.clear();
                    ui.set_theme(colors);
                    self.save_settings();
                }

                Err(error) => self.error = format!("{error:#}"),
            },
            Action::DiscoverFonts => self.system_fonts.start(platform.waker())?,

            Action::Font(name) => {
                let font = name
                    .map(|name| {
                        self.system_fonts
                            .families
                            .iter()
                            .find(|family| family.name == name)
                            .cloned()
                            .ok_or_else(|| anyhow::anyhow!("Font is unavailable"))
                    })
                    .transpose()?;
                ui.set_system_font(font.as_ref())?;
                self.system_fonts.select(font.as_ref());
                self.font = font;
                self.chat.invalidate_history_layout();
                self.error.clear();
                self.save_settings();
            }
        }

        let previous_tab = previous.page.tab();
        if previous_tab != self.page.tab() {
            self.tab_locations[previous_tab as usize] = if creating_profile {
                None
            } else {
                Some(TabLocation {
                    location: previous,
                    contact_mode: match previous_contact_mode {
                        contacts_ui::Mode::Add | contacts_ui::Mode::Edit => previous_contact_mode,
                        _ => contacts_ui::Mode::View,
                    },
                })
            };
        }

        if self.location() != previous {
            self.contact_delete_in_list = None;
        }

        if self.chat.renaming.is_some() && !begin_rename && self.location() != previous {
            self.chat.renaming = None;
            ui.clear_input(chat_ui::RENAME_INPUT);
        }

        if (self.location() != previous || record_history) && !focus_requested {
            if self.page == Page::Create {
                ui.focus_control("new-profile-handle", true);
            } else if self.page == Page::CreateUsername {
                ui.focus_control("new-profile-username", true);
            } else if self.page == Page::CreateConversation {
                if self.chat.naming {
                    ui.focus_input(&self.chat.title_id());
                } else {
                    ui.focus_input(&self.chat.partner_id());
                }
            } else if self.page.tab() == Tab::Contacts
                && self.contact_mode == contacts_ui::Mode::Add
            {
                ui.focus_input("contact-auth");
            } else if self.page.tab() == Tab::Contacts
                && self.contact_mode == contacts_ui::Mode::Edit
            {
                ui.focus_input("contact-name");
            } else {
                ui.navigation_changed();
            }
        }

        if record_history && self.location() != previous {
            self.back_history.push(previous);
            self.forward_history.clear();
        }

        platform.request_redraw()?;
        Ok(())
    }
}

// A pane with its own layout and clipping region.
fn panel(ui: &mut Framework, x: f32, y: f32, w: f32, h: f32, surface: bool) -> anyhow::Result<()> {
    panel_named(ui, None, [x, y, w, h], surface)
}

fn panel_named(
    ui: &mut Framework,
    id: Option<&str>,
    bounds: [f32; 4],
    surface: bool,
) -> anyhow::Result<()> {
    let [x, y, w, h] = bounds;
    ui.set_position(x, y);
    ui.push_style(Style {
        background: Some(if surface {
            ui.theme().surface
        } else {
            ui.theme().background
        }),
        ..ui.style()
    });

    let options = DivOptions {
        width: Some(w.max(0.0)),
        height: Some(h.max(0.0)),
        ..Default::default()
    };

    if let Some(id) = id {
        ui.div_begin_id(id, options)?;
    } else {
        ui.div_begin(options)?;
    }

    Ok(())
}

// Restore the parent pane and its style.
fn end_panel(ui: &mut Framework) {
    ui.div_end();
    ui.pop_style();
}

// Text using the current theme and the requested weight.
fn empty_state(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    icon: &str,
    message: &str,
) -> anyhow::Result<()> {
    let top = (ui.div_height() * 0.42 - 36.0).max(ui.position().1 + 24.0);
    ui.set_position(0.0, top);
    ui.div_begin_id(
        id,
        DivOptions {
            width: Some(ui.div_width()),
            ..Default::default()
        },
    )?;
    ui.push_style(Style {
        font_size: 36,
        color: ui.theme().muted,
        ..ui.style()
    });
    ui.text_centered(gpu, icon, Some(Font::Icons))?;
    ui.pop_style();
    ui.space(16.0);
    label(ui, gpu, message, 14, false, true, true)?;
    ui.div_end();
    Ok(())
}

fn label(
    ui: &mut Framework,
    gpu: &mut Renderer,
    value: &str,
    size: u32,
    bold: bool,
    muted: bool,
    centered: bool,
) -> anyhow::Result<()> {
    ui.push_style(Style {
        font_size: size,
        color: if muted {
            ui.theme().muted
        } else {
            ui.theme().text
        },
        ..ui.style()
    });

    let font = Some(if bold {
        Font::Bold
    } else {
        Font::Regular
    });

    let result = if centered {
        ui.text_centered(gpu, value, font)
    } else {
        ui.text(gpu, value, font)
    };

    ui.pop_style();
    result
}

// Centered icon button for compact controls.
fn icon(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    glyph: &str,
    selected: bool,
    tooltip: &'static str,
) -> anyhow::Result<bool> {
    ui.push_style(Style {
        selected,
        tooltip: Some(tooltip),
        selection_group: id.starts_with("nav-").then_some("navigation"),
        font_size: 20,
        padding: 4.0,
        radius: ui.corner_radii().icon,
        color: if selected {
            ui.theme().selected_icon
        } else {
            ui.theme().text
        },
        background: Some(if selected {
            ui.theme().button
        } else {
            ui.theme().surface
        }),
        ..ui.style()
    });

    ui.push_sizing(Some(40.0), Some(40.0));
    let result = ui.icon_button(gpu, id, glyph);
    ui.pop_sizing();
    ui.pop_style();
    result
}

fn header_icon(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    glyph: &str,
    tooltip: &'static str,
    size: f32,
) -> anyhow::Result<bool> {
    ui.push_style(Style {
        tooltip: Some(tooltip),
        font_size: ((size * 0.5) as u32).max(14),
        padding: 4.0,
        radius: ui.corner_radii().icon,
        color: ui.theme().text,
        background: None,
        ..ui.style()
    });
    ui.push_sizing(Some(size), Some(size));
    let clicked = ui.icon_button(gpu, id, glyph);
    ui.pop_sizing();
    ui.pop_style();
    clicked
}

enum TitleEdit {
    None,
    Cancel,
    Submit(String),
}

const INLINE_TITLE_MIN_HEIGHT: f32 = 26.0;

fn inline_title_editor(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    hint: &str,
    typography: (Font, u32),
    max_chars: usize,
    allow_empty: bool,
) -> anyhow::Result<TitleEdit> {
    let (font, font_size) = typography;
    let button = (font_size as f32 * 1.5 + 1.0).max(INLINE_TITLE_MIN_HEIGHT);
    let width = ui.div_width();
    let y = ui.position().1;
    ui.push_style(Style {
        font_size,
        padding: 0.0,
        color: ui.theme().text,
        background: None,
        border_color: None,
        ..ui.style()
    });
    ui.push_sizing(Some((width - button * 2.0 - 16.0).max(1.0)), Some(button));
    let name = ui.input_with_font(
        gpu,
        id,
        InputOptions {
            max_chars: Some(max_chars),
            filter: Some(|c| !c.is_control()),
        },
        hint,
        font,
    );
    ui.pop_sizing();
    ui.pop_style();
    let name = name?;

    ui.set_position((width - button * 2.0 - 8.0).max(0.0), y);
    let cancel = header_icon(
        ui,
        gpu,
        "cancel-inline-title",
        "\u{ea76}",
        "Cancel edit",
        button,
    )?;
    ui.set_position((width - button).max(0.0), y);
    let can_save = allow_empty || !name.trim().is_empty();
    ui.push_style(Style {
        enabled: can_save,
        ..ui.style()
    });
    let save = header_icon(
        ui,
        gpu,
        "save-inline-title",
        "\u{eab2}",
        "Save name",
        button,
    );
    ui.pop_style();
    let save = save?;

    if cancel
        || ui.events().iter().any(|event| {
            matches!(
                event,
                WindowEvent::KeyPressed {
                    key: Key::Escape,
                    ..
                }
            )
        })
    {
        Ok(TitleEdit::Cancel)
    } else if can_save && (save || ui.input_submitted(id)) {
        Ok(TitleEdit::Submit(name))
    } else {
        Ok(TitleEdit::None)
    }
}

// Full-width button with an icon and an optional selected badge.
fn row_button(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    glyph: &str,
    value: &str,
    selected: bool,
    primary: bool,
) -> anyhow::Result<bool> {
    let glyph = if selected && glyph == ACCOUNTS {
        ACCOUNTS_SELECTED
    } else {
        glyph
    };

    ui.push_style(Style {
        selected,
        selection_group: (!primary).then_some("list-selection"),
        font_size: 16,
        icon_font_size: (glyph == ACCOUNTS || glyph == ACCOUNTS_SELECTED).then_some(28),
        padding: 14.0,
        hit_padding: 5.0,
        radius: ui.corner_radii().row,
        color: if !ui.style().enabled {
            ui.theme().muted
        } else if primary {
            ui.theme().primary_text
        } else if selected {
            ui.theme().selected_icon
        } else {
            ui.theme().button_text
        },
        background: Some(if !ui.style().enabled {
            ui.theme().button
        } else if primary {
            ui.theme().primary
        } else if selected {
            ui.theme().button
        } else {
            ui.theme().surface
        }),
        ..ui.style()
    });

    ui.push_sizing(Some(ui.div_width()), Some(48.0));
    let result = ui.icon_label_badge_button(
        gpu,
        id,
        glyph,
        value,
        (selected && glyph == ACCOUNTS_SELECTED).then_some("Selected"),
    );
    ui.pop_sizing();
    ui.pop_style();
    result
}

// The collapsible rail for accounts, chats and settings.
fn navigation(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    height: f32,
) -> anyhow::Result<Action> {
    let mut action = Action::None;
    ui.set_position(16.0, 16.0);

    if icon(ui, gpu, "nav-collapse", MENU, false, "Hide navigation")? {
        action = Action::ToggleMenu;
    }

    ui.set_position(16.0, 88.0);
    // The 40px icons are 56px apart; split the gap at its midpoint.
    ui.push_style(Style {
        hit_padding: 8.0,
        ..ui.style()
    });
    let accounts_selected = matches!(
        app.page,
        Page::Accounts | Page::Create | Page::CreateUsername
    );

    if icon(
        ui,
        gpu,
        "nav-accounts",
        if accounts_selected {
            ACCOUNTS_SELECTED
        } else {
            ACCOUNTS
        },
        accounts_selected,
        "Profiles",
    )? {
        action = Action::Choose;
    }

    ui.set_position(16.0, 144.0);

    let chats_selected = app.page.is_chat();

    if icon(
        ui,
        gpu,
        "nav-chats",
        if chats_selected {
            CHATS_SELECTED
        } else {
            CHATS
        },
        chats_selected,
        "Conversations",
    )? {
        action = Action::Chats;
    }
    ui.set_position(16.0, 200.0);
    let contacts_selected = matches!(app.page, Page::Contacts(_));

    if icon(
        ui,
        gpu,
        "nav-contacts",
        if contacts_selected {
            contacts::ICON
        } else {
            "󰖸"
        },
        contacts_selected,
        "Contacts",
    )? {
        action = Action::Contacts;
    }

    ui.pop_style();

    // Keep settings at the bottom of the rail.
    ui.set_position(16.0, (height - 64.0).max(256.0));

    if icon(
        ui,
        gpu,
        "nav-settings",
        if app.page == Page::Settings {
            SETTINGS_SELECTED
        } else {
            SETTINGS
        },
        app.page == Page::Settings,
        "Settings",
    )? {
        action = Action::Settings;
    }

    Ok(action)
}

// Shared heading, menu toggle and overflow button.
fn list_header(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    title: &str,
) -> anyhow::Result<(Action, f32)> {
    let mut action = Action::None;
    let width = ui.div_width();
    ui.set_position(0.0, 0.0);
    ui.div_begin(DivOptions {
        width: Some(width),
        ..Default::default()
    })?;

    // Keep the menu toggle available when the rail is hidden.
    if app.collapsed {
        ui.set_position(12.0, 16.0);

        if icon(ui, gpu, "list-expand", MENU, false, "Show navigation")? {
            action = Action::ToggleMenu;
        }
    }

    let chats = app.page.is_chat();
    let title_x = if app.collapsed {
        62.0
    } else {
        24.0
    };
    let actions_width = if chats {
        96.0
    } else {
        52.0
    };
    ui.set_position(
        title_x,
        if chats {
            25.0
        } else {
            22.0
        },
    );
    ui.push_sizing(Some((width - title_x - actions_width - 4.0).max(1.0)), None);
    label(
        ui,
        gpu,
        title,
        if chats {
            16
        } else {
            20
        },
        true,
        false,
        false,
    )?;
    ui.pop_sizing();
    ui.set_position(width - actions_width, 16.0);
    if icon(ui, gpu, "list-more", MORE, false, "More options")? {
        action = Action::OpenMenu;
    }

    if chats {
        ui.set_position(width - 52.0, 16.0);
        ui.push_style(Style {
            enabled: app.selected.is_some(),
            ..ui.style()
        });
        let create = icon(
            ui,
            gpu,
            "list-new-chat",
            NEW_CHAT,
            false,
            "New conversation",
        )?;
        ui.pop_style();
        if create && let Some(profile) = &app.selected {
            action = Action::NewConversation(profile.clone());
        }
    }
    ui.div_end();
    ui.space(38.0);
    Ok((action, ui.position().1))
}

// The available settings sections.
fn settings_list(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    height: f32,
) -> anyhow::Result<Action> {
    let (mut action, list_top) = list_header(ui, gpu, app, "Settings")?;
    let width = ui.div_width();
    panel(
        ui,
        16.0,
        list_top,
        width - 32.0,
        (height - list_top - 16.0).max(0.0),
        true,
    )?;

    if row_button(
        ui,
        gpu,
        "settings-appearance",
        THEME,
        "Appearance",
        app.settings_item == SettingsItem::Appearance,
        false,
    )? {
        action = Action::SettingsItem(SettingsItem::Appearance);
    }

    ui.space(10.0);

    if row_button(
        ui,
        gpu,
        "settings-misc",
        "\u{eb52}",
        "Misc",
        app.settings_item == SettingsItem::Misc,
        false,
    )? {
        action = Action::SettingsItem(SettingsItem::Misc);
    }

    end_panel(ui);
    Ok(action)
}

// Profile and conversation rows in the left-hand list.
fn item_list(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    snapshot: &client::Snapshot,
    height: f32,
) -> anyhow::Result<Action> {
    let (mut action, mut list_top) = list_header(
        ui,
        gpu,
        app,
        if app.page.is_chat() {
            "Conversations"
        } else {
            "Profiles"
        },
    )?;
    let width = ui.div_width();
    let chats = app.page.is_chat();

    if chats && let Some(profile) = &app.selected {
        panel_named(
            ui,
            Some("request-navigation"),
            [16.0, list_top, width - 32.0, 56.0],
            true,
        )?;
        let detail = if snapshot.requests.is_empty() {
            "No pending requests"
        } else {
            "Incoming requests"
        };
        let badge =
            (!snapshot.requests.is_empty()).then(|| snapshot.requests.len().min(99).to_string());
        if ui
            .action_row(
                gpu,
                "message-requests",
                ui::framework::ActionRow {
                    icon: REQUESTS,
                    title: "Message requests",
                    detail,
                    selected: app.page == Page::Requests,
                    badge: badge.as_deref(),
                },
            )?
            .clicked
        {
            action = Action::MessageRequests(profile.clone());
        }

        if !snapshot.pending_invites.is_empty() {
            let count = snapshot.pending_invites.len().min(99).to_string();
            if ui
                .action_row(
                    gpu,
                    "pending-invites",
                    ui::framework::ActionRow {
                        icon: REQUESTS,
                        title: "Pending invites",
                        detail: "Outgoing conversation requests",
                        selected: app.page == Page::PendingInvites,
                        badge: Some(&count),
                    },
                )?
                .clicked
            {
                action = Action::PendingInvites(profile.clone());
            }
        }

        end_panel(ui);
        list_top += 60.0;
    }

    let count = if chats {
        snapshot.conversations.len()
    } else {
        app.profiles.len()
    };

    // Fit as many rows as the current window height allows.
    let per_page = ((height - list_top - 156.0) / 58.0).floor().max(1.0) as usize;
    let mut offset = app
        .list_offset
        .min(count.saturating_sub(1) / per_page * per_page);
    if chats
        && let Some(id) = app.chat.renaming
        && let Some(index) = snapshot.conversations.iter().position(|chat| chat.id == id)
        && !(offset..offset + per_page).contains(&index)
    {
        offset = index / per_page * per_page;
    }

    panel_named(
        ui,
        Some(if chats {
            "conversation-list"
        } else {
            "profile-list"
        }),
        [
            16.0,
            list_top,
            width - 32.0,
            (height - list_top - 132.0).max(0.0),
        ],
        true,
    )?;

    // An empty list still gets a heading and a short explanation.
    if chats && app.session.is_some() && snapshot.auth.is_none() {
        ui.set_position(8.0, 14.0);
        let status = if snapshot.generation == 0 {
            "Loading conversations..."
        } else {
            &snapshot.status
        };
        label(ui, gpu, status, 14, false, true, false)?;
    } else if count == 0 {
        ui.set_position(8.0, 14.0);
        ui.div_begin(DivOptions {
            width: Some((ui.div_width() - 16.0).max(1.0)),
            ..Default::default()
        })?;
        if chats {
            label(ui, gpu, "No conversations yet", 16, true, false, false)?;
            ui.space(12.0);
            if let Some(profile) = &app.selected
                && row_button(ui, gpu, "new-chat-empty", NEW_CHAT, "New chat", false, true)?
            {
                action = Action::NewConversation(profile.clone());
            }
        } else {
            label(ui, gpu, "No profile yet", 16, true, false, false)?;
            ui.space(12.0);
            label(
                ui,
                gpu,
                "Create your first profile to begin chatting.",
                14,
                false,
                true,
                false,
            )?;
        }

        ui.div_end();
    } else {
        // Render one row for each visible profile or conversation.
        for index in offset..(offset + per_page).min(count) {
            ui.set_position(0.0, (index - offset) as f32 * 58.0);

            if chats {
                let chat = &snapshot.conversations[index];
                let id = chat.id;
                let (x, y) = ui.position();
                if let Some(profile) = &app.selected {
                    let editing = app.chat.renaming == Some(id);
                    let title = if editing {
                        std::borrow::Cow::Borrowed("")
                    } else {
                        app.conversation_title(chat)
                    };
                    ui.push_style(Style {
                        enabled: !editing,
                        ..ui.style()
                    });
                    let row = ui.action_row(
                        gpu,
                        &format!("conversation-{id}"),
                        ui::framework::ActionRow {
                            title: &title,
                            detail: &chat.last,
                            icon: GROUP,
                            selected: app.conversation == Some(id)
                                && app.page == Page::Conversations,
                            badge: None,
                        },
                    );
                    ui.pop_style();
                    let clicked = row?.clicked;

                    if editing {
                        let width = (ui.div_width() - 48.0).max(1.0);
                        ui.set_position(44.0, y + 3.0);
                        ui.div_begin(DivOptions {
                            width: Some(width),
                            height: Some(INLINE_TITLE_MIN_HEIGHT),
                            ..Default::default()
                        })?;
                        let result = inline_title_editor(
                            ui,
                            gpu,
                            chat_ui::RENAME_INPUT,
                            "Name",
                            (Font::Regular, 14),
                            client::CONVERSATION_NAME_MAX,
                            false,
                        );
                        ui.div_end();
                        action = match result? {
                            TitleEdit::Cancel => Action::CancelConversationRename,
                            TitleEdit::Submit(_) => Action::RenameConversation {
                                profile: profile.clone(),
                                id,
                            },
                            TitleEdit::None => action,
                        };
                    } else if let Some(at) = ui.context_press([x, y, ui.div_width(), 56.0]) {
                        action = Action::OpenRowMenu(
                            RowMenu::Conversation {
                                profile: profile.clone(),
                                id,
                            },
                            at,
                        );
                    } else if clicked {
                        action = Action::OpenConversation {
                            profile: profile.clone(),
                            id,
                        };
                    }
                }
            } else {
                let profile = &app.profiles[index];
                let selected = app
                    .selected
                    .as_ref()
                    .is_some_and(|p| p.path == profile.path);

                let (x, y) = ui.position();
                let clicked = row_button(
                    ui,
                    gpu,
                    &format!("profile-{}", profile.alias),
                    ACCOUNTS,
                    &profile.alias,
                    selected,
                    false,
                )?;
                if let Some(at) = ui.context_press([x, y, ui.div_width(), 48.0]) {
                    action = Action::OpenRowMenu(RowMenu::Profile(profile.clone()), at);
                } else if clicked {
                    action = Action::Select(index);
                }
            }
        }
    }

    end_panel(ui);

    // Show paging controls when the list does not fit.
    if count > per_page {
        ui.set_position(16.0, height - 126.0);

        if offset > 0 && icon(ui, gpu, "list-previous", BACK, false, "Previous page")? {
            action = Action::ListPage(offset.saturating_sub(per_page));
        }

        ui.set_position(width - 56.0, height - 126.0);

        if offset + per_page < count && icon(ui, gpu, "list-next", NEXT, false, "Next page")? {
            action = Action::ListPage(offset + per_page);
        }
    }

    // The list footer shows account status or the create button.
    let footer_y = height - 70.0;
    panel_named(
        ui,
        Some("profile-footer"),
        [16.0, footer_y, width - 32.0, 54.0],
        true,
    )?;

    if chats {
        if let Some(profile) = &app.selected {
            let copy_x = (ui.div_width() - 40.0).max(0.0);
            let has_auth = snapshot.auth.is_some();
            if has_auth {
                ui.push_sizing(Some((copy_x - 8.0).max(1.0)), None);
            }

            let hovered = ui.div_hovered();
            label(
                ui,
                gpu,
                snapshot.username.as_deref().unwrap_or(&profile.alias),
                15,
                true,
                false,
                false,
            )?;
            if snapshot.authorized {
                let reveal = if app.layout_morphing && app.morph_strength > 0 {
                    ui.presence(&mut app.chat.footer_hover.borrow_mut(), hovered)
                } else {
                    f32::from(hovered)
                };
                let width = if has_auth {
                    (copy_x - 8.0).max(1.0)
                } else {
                    ui.div_width()
                };
                let y = ui.position().1;

                if reveal < 1.0 {
                    footer_label(
                        ui,
                        gpu,
                        "profile-connected",
                        "Connected to server",
                        [width, y],
                        1.0 - reveal,
                    )?;
                }

                if reveal > 0.0 {
                    footer_label(
                        ui,
                        gpu,
                        "profile-hover",
                        &format!("Profile: {}", profile.alias),
                        [width, y],
                        reveal,
                    )?;
                }
            } else {
                let status = if snapshot.status.is_empty() {
                    "Connecting…"
                } else {
                    &snapshot.status
                };
                label(ui, gpu, status, 12, false, true, false)?;
            }

            if has_auth {
                ui.pop_sizing();
                ui.set_position(copy_x, 7.0);
                if icon(ui, gpu, "copy-auth-id", COPY_AUTH, false, "Copy Auth ID")? {
                    action = Action::CopyAuthId(profile.clone());
                }
            }
        } else if row_button(
            ui,
            gpu,
            "choose-profile",
            ACCOUNTS,
            "Choose a profile",
            false,
            false,
        )? {
            action = Action::Choose;
        }
    } else if row_button(
        ui,
        gpu,
        "create-profile",
        ADD,
        "Create New...",
        false,
        false,
    )? {
        action = Action::New;
    }

    end_panel(ui);
    Ok(action)
}

fn footer_label(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    text: &str,
    [width, y]: [f32; 2],
    reveal: f32,
) -> anyhow::Result<()> {
    ui.set_position(0.0, y);
    ui.div_begin_reveal_from_left(
        id,
        DivOptions {
            width: Some(width),
            height: Some(18.0),
            ..Default::default()
        },
        reveal,
    )?;
    let muted = ui.theme().muted;
    ui.push_style(Style {
        font_size: 12,
        color: graphics::Color {
            a: muted.a * reveal,
            ..muted
        },
        ..ui.style()
    });
    let result = ui.text(gpu, text, None);
    ui.pop_style();
    ui.div_end();
    result
}

/* we give the history arrows plenty of room to click */
fn history_controls(ui: &mut Framework, gpu: &mut Renderer, app: &App) -> anyhow::Result<Action> {
    let can_back = !app.back_history.is_empty() || app.page != Page::Accounts;
    let mut action = Action::None;
    for (id, glyph, x, enabled, next) in [
        ("history-back", BACK, 20.0, can_back, Action::Back),
        (
            "history-forward",
            NEXT,
            72.0,
            !app.forward_history.is_empty(),
            Action::Forward,
        ),
    ] {
        if id == "history-forward" && !enabled {
            continue;
        }

        ui.set_position(x, 14.0);
        let mut color = ui.theme().text;
        if !enabled {
            color.a *= 0.3;
        }

        ui.push_style(Style {
            tooltip: Some(if id == "history-back" {
                "Back"
            } else {
                "Forward"
            }),
            font_size: 20,
            padding: 8.0,
            hit_padding: 4.0,
            enabled,
            color,
            radius: ui.corner_radii().control,
            background: Some(ui.theme().background),
            hover_color: enabled.then_some(ui::widgets::button::highlight(
                ui.theme().button,
                ui.theme().button_text,
            )),
            hover_text_color: enabled.then_some(ui.theme().button_text),
            click_color: Some(ui.theme().primary),
            click_text_color: Some(ui.theme().primary_text),
            ..ui.style()
        });
        ui.push_sizing(Some(44.0), Some(44.0));
        let clicked = ui.icon_button(gpu, id, glyph);
        ui.pop_sizing();
        ui.pop_style();
        if clicked? && enabled {
            action = next;
        }
    }

    Ok(action)
}

// The centered logo, name and tagline.
fn brand_page(
    ui: &mut Framework,
    gpu: &mut Renderer,
    logo: Image,
    first_profile: bool,
) -> anyhow::Result<Action> {
    let width = ui.div_width();
    let height = ui.div_height();
    let side_padding = (width * 0.12).clamp(24.0, 96.0);
    let content_width = (width - 2.0 * side_padding).clamp(1.0, 640.0);
    let size = 100.0_f32.min(content_width);
    let motivation = "Faith lets you create disposable profiles without creating links to your identity. Talk securely in a fast, native application that just works.";
    let quote = "When faith is abandoned, software decays.";
    let version = concat!("v", env!("CARGO_PKG_VERSION"));
    let mut content_height = size + 32.0 + 12.0 + 12.0 + 20.0;
    if first_profile {
        content_height += 24.0 + 48.0;
    }
    for (text, font_size, font) in [
        ("faith.", 30, Font::Bold),
        (motivation, 15, Font::Regular),
        (quote, 15, Font::Italic),
        (version, 12, Font::Regular),
    ] {
        ui.push_style(Style {
            font_size,
            ..ui.style()
        });
        let measured = ui.wrapped_text_size(text, Some(font), content_width);
        ui.pop_style();
        content_height += measured?.1;
    }
    let top = ((height - content_height) * 0.5).max(24.0);
    ui.set_position(((width - content_width) * 0.5).max(0.0), top);
    ui.div_begin(DivOptions {
        width: Some(content_width),
        ..Default::default()
    })?;

    ui.set_position((content_width - size) * 0.5, 0.0);

    // Center the logo inside the text column.
    ui.image(logo, size, size)?;

    // Let wrapped headings push the tagline down.
    ui.space(32.0);
    label(ui, gpu, "faith.", 30, true, false, true)?;
    ui.space(12.0);
    label(ui, gpu, motivation, 15, false, true, true)?;
    // Keep the quote distinct from the motivation text.
    ui.space(12.0);
    ui.push_style(Style {
        font_size: 15,
        color: ui.theme().muted,
        ..ui.style()
    });
    let result = ui.text_centered(gpu, quote, Some(Font::Italic));
    ui.pop_style();
    result?;
    let mut action = Action::None;
    if first_profile {
        ui.space(24.0);
        let button_width = content_width.min(260.0);
        let y = ui.position().1;
        ui.set_position((content_width - button_width) * 0.5, y);
        ui.push_style(Style {
            font_size: 16,
            padding: 12.0,
            radius: ui.corner_radii().control,
            background: Some(ui.theme().primary),
            color: ui.theme().primary_text,
            center_button_content: true,
            ..ui.style()
        });
        ui.push_sizing(Some(button_width), Some(48.0));
        let clicked = ui.icon_label_button(gpu, "first-profile", ADD, "Create profile");
        ui.pop_sizing();
        ui.pop_style();
        if clicked? {
            action = Action::New;
        }
    }
    // Keep the installed version quiet and separate from the tagline.
    ui.space(20.0);
    label(ui, gpu, version, 12, false, true, true)?;
    ui.div_end();
    Ok(action)
}

// The form for creating a local account profile.
fn profile_input_style(ui: &Framework) -> Style {
    Style {
        padding: 12.0,
        font_size: 16,
        radius: ui.corner_radii().control,
        border_color: Some(ui.theme().border),
        background: Some(ui.theme().input),
        ..ui.style()
    }
}

fn profile_creation_page(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
) -> anyhow::Result<Action> {
    // Keep the form narrow and centered in the main pane.
    let width = (ui.div_width() - 64.0).clamp(1.0, 380.0);
    let x = (ui.div_width() - width) * 0.5;
    let y = ((ui.div_height() - 540.0) * 0.5).max(24.0);
    ui.set_position(x, y);
    ui.push_style(Style {
        background: Some(ui.theme().background),
        ..ui.style()
    });

    let username_page = app.page == Page::CreateUsername;
    let form = &app.profile_form;
    ui.div_begin_id(
        "profile-creation",
        DivOptions {
            width: Some(width),
            height: Some((ui.div_height() - y - 54.0).max(1.0)),
            scrollable: true,
        },
    )?;

    ui.push_style(Style {
        font_size: 28,
        color: ui.theme().muted,
        ..ui.style()
    });

    ui.text(gpu, LOCK, Some(Font::Icons))?;
    ui.pop_style();
    ui.space(20.0);
    label(
        ui,
        gpu,
        if username_page {
            "Create user for profile"
        } else if !app.profiles.is_empty() {
            "Create a new profile"
        } else {
            "Create your first profile"
        },
        26,
        true,
        false,
        false,
    )?;
    ui.div_begin_id(
        if username_page {
            "profile-username-fields"
        } else {
            "profile-handle-fields"
        },
        DivOptions {
            width: Some(ui.div_width()),
            ..Default::default()
        },
    )?;

    if username_page {
        let action = profile_username_form(ui, gpu, form)?;
        ui.div_end();
        end_panel(ui);
        return Ok(action);
    }

    ui.space(12.0);
    label(
        ui,
        gpu,
        "Enter the local alias for your account. You can create unlimited profiles.",
        15,
        false,
        true,
        false,
    )?;
    ui.space(28.0);
    label(ui, gpu, "Profile name", 13, true, true, false)?;
    ui.space(10.0);
    ui.push_style(profile_input_style(ui));

    // Only allow characters that are valid in a profile handle.
    let alias = ui.input_with_options(
        gpu,
        "new-profile-handle",
        InputOptions {
            max_chars: Some(20),
            filter: Some(|c| c.is_ascii_alphanumeric() || c == '_'),
        },
    )?;
    ui.pop_style();
    ui.space(8.0);
    label(
        ui,
        gpu,
        "Letters, numbers and underscores. Up to 20 characters.",
        12,
        false,
        true,
        false,
    )?;
    ui.space(24.0);
    let mut action = Action::None;

    let y = ui.position().1;
    ui.push_sizing(Some((width - 42.0).max(1.0)), None);
    ui.push_style(Style {
        font_size: 13,
        ..ui.style()
    });
    let claim = ui.checkbox_with_suffix(
        gpu,
        "claim-username",
        "Claim username",
        Some("(optional)"),
        form.claim,
    );
    ui.pop_style();

    if let Some(claim) = claim? {
        action = Action::ClaimUsername(claim);
    }

    ui.pop_sizing();
    ui.set_position((width - 40.0).max(0.0), (y + ui.position().1 - 40.0) * 0.5);
    ui.push_style(Style {
        tooltip: Some("How usernames work"),
        font_size: 20,
        padding: 4.0,
        radius: ui.corner_radii().icon,
        color: ui.theme().muted,
        background: None,
        ..ui.style()
    });
    ui.push_sizing(Some(40.0), Some(40.0));
    let help_clicked = ui.icon_button(gpu, "username-help", "");
    ui.pop_sizing();
    ui.pop_style();

    if help_clicked? {
        action = Action::ToggleUsernameHelp;
    }

    disclosure_text(
        ui,
        gpu,
        "username-help-text",
        "Ticking this will let you claim a username for your profile. Usernames are globally unique and can be used to identify you. If left unticked, this profile will be anonymous, letting others only ever see your randomly generated Auth ID.",
        form.help,
    )?;

    ui.space(24.0);
    let can_create = profiles::valid_alias(&alias);
    ui.push_style(Style {
        enabled: can_create,
        ..ui.style()
    });
    let clicked = row_button(
        ui,
        gpu,
        "submit-create",
        if form.claim {
            NEXT
        } else {
            ADD
        },
        if form.claim {
            "Next"
        } else {
            "Create profile"
        },
        false,
        true,
    )?;
    ui.pop_style();

    if can_create && (clicked || ui.input_submitted("new-profile-handle")) {
        action = if form.claim {
            Action::ProfileUsername(alias)
        } else {
            Action::Create(alias)
        };
    }

    ui.div_end();
    end_panel(ui);
    Ok(action)
}

fn disclosure_text(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    text: &str,
    open: bool,
) -> anyhow::Result<()> {
    if open {
        ui.div_begin_id(
            id,
            DivOptions {
                width: Some(ui.div_width()),
                ..Default::default()
            },
        )?;
        ui.space(8.0);
        label(ui, gpu, text, 13, false, true, false)?;
        ui.div_end();
    }

    Ok(())
}

fn profile_username_form(
    ui: &mut Framework,
    gpu: &mut Renderer,
    form: &ProfileForm,
) -> anyhow::Result<Action> {
    let mut action = Action::None;
    let editing = form.pending.is_none();
    ui.space(28.0);
    label(ui, gpu, "Username", 13, true, true, false)?;
    ui.space(10.0);
    ui.push_style(Style {
        enabled: editing,
        ..profile_input_style(ui)
    });
    let username = ui.input_with_options(
        gpu,
        "new-profile-username",
        InputOptions {
            max_chars: Some(profiles::USERNAME_MAX),
            filter: Some(profiles::username_char),
        },
    )?;
    ui.pop_style();
    ui.space(8.0);
    label(ui, gpu, profiles::USERNAME_ERROR, 12, false, true, false)?;
    ui.space(16.0);

    ui.push_style(Style {
        enabled: editing,
        ..ui.style()
    });
    let advanced = advanced_button(ui, gpu, "username-advanced", form.advanced && editing);
    ui.pop_style();

    if advanced? {
        action = Action::ToggleUsernameAdvanced;
    }

    if editing && form.advanced {
        ui.div_begin_id(
            "username-preferences",
            DivOptions {
                width: Some(ui.div_width()),
                ..Default::default()
            },
        )?;
        ui.space(8.0);
        ui.push_style(Style {
            font_size: 13,
            ..ui.style()
        });
        if let Some(value) = ui.compact_checkbox(
            gpu,
            "auto-disclose-username",
            "Disclose username automatically",
            form.username.auto_disclose,
        )? {
            action = Action::AutoDiscloseUsername(value);
        }

        ui.set_position(30.0, ui.position().1);
        label(
            ui,
            gpu,
            "Share your username when accepting optional requests and preselect sharing on new invitations.",
            12,
            false,
            true,
            false,
        )?;
        ui.space(12.0);
        if let Some(value) = ui.compact_checkbox(
            gpu,
            "reject-required-username",
            "Automatically reject requests that require username disclosure",
            form.username.reject_required,
        )? {
            action = Action::RejectRequiredUsername(value);
        }

        ui.set_position(30.0, ui.position().1);
        label(
            ui,
            gpu,
            "Decline invitations that require you to share your username.",
            12,
            false,
            true,
            false,
        )?;
        ui.pop_style();
        ui.div_end();
    }

    ui.space(24.0);
    ui.push_style(Style {
        enabled: editing,
        ..ui.style()
    });
    let clicked = row_button(
        ui,
        gpu,
        "submit-username",
        ADD,
        if editing {
            "Create profile"
        } else {
            "Claiming username…"
        },
        false,
        true,
    )?;
    ui.pop_style();

    if editing && (clicked || ui.input_submitted("new-profile-username")) {
        action = Action::CreateWithUsername(username);
    }

    Ok(action)
}

fn advanced_button(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    expanded: bool,
) -> anyhow::Result<bool> {
    ui.push_style(Style {
        font_size: 13,
        icon_font_size: Some(16),
        padding: 6.0,
        color: ui.theme().muted,
        background: None,
        border_color: None,
        radius: ui.corner_radii().control,
        selected: false,
        selection_group: None,
        ..ui.style()
    });
    ui.push_sizing(Some(ui.div_width().min(200.0)), Some(32.0));
    let clicked = ui.icon_label_button(
        gpu,
        id,
        if expanded {
            "\u{f0143}"
        } else {
            "\u{f0140}"
        },
        "Advanced options",
    );
    ui.pop_sizing();
    ui.pop_style();
    clicked
}

fn misc_settings_page(ui: &mut Framework, gpu: &mut Renderer, app: &App) -> anyhow::Result<Action> {
    let width = (ui.div_width() - 64.0).max(1.0);
    let height = (ui.div_height() - 112.0).max(1.0);
    ui.set_position(32.0, 36.0);
    ui.push_style(Style {
        background: Some(ui.theme().background),
        ..ui.style()
    });
    ui.div_begin_id(
        "settings-misc",
        DivOptions {
            width: Some(width),
            height: Some(height),
            scrollable: true,
        },
    )?;
    label(ui, gpu, "Misc", 26, true, false, false)?;
    ui.space(12.0);
    label(
        ui,
        gpu,
        "Adjust details in the user interface.",
        15,
        false,
        true,
        false,
    )?;
    ui.space(36.0);
    label(ui, gpu, "Interface", 13, true, true, false)?;
    ui.space(10.0);

    // Apply the rounded-corner preference when the checkbox changes.
    let change = ui.checkbox(
        gpu,
        "settings-rounded-corners",
        "UI rounded corners",
        app.rounded,
    )?;
    ui.space(8.0);
    ui.push_sizing(Some(ui.div_width().min(340.0)), None);
    label(
        ui,
        gpu,
        "Adds rounded corners to elements in the interface. When enabled, you can also enable layout morphing.",
        13,
        false,
        true,
        false,
    )?;
    ui.pop_sizing();
    ui.space(24.0);
    let animations = ui.checkbox(gpu, "settings-animations", "UI animations", app.animations)?;
    let mut toggle_morph_advanced = false;
    let mut morph_strength = None;
    let mut morph_duration = None;
    let mut morph_curve = None;
    let layout_morphing = if app.animations {
        ui.space(8.0);
        let y = ui.position().1;
        ui.set_position(20.0, y);
        ui.div_begin_id(
            "animation-details",
            DivOptions {
                width: Some((ui.div_width() - 20.0).max(1.0)),
                ..Default::default()
            },
        )?;
        let change = ui.checkbox(
            gpu,
            "settings-layout-morphing",
            "Layout morphing animations",
            app.layout_morphing,
        )?;
        ui.space(8.0);
        toggle_morph_advanced =
            advanced_button(ui, gpu, "settings-morph-advanced", app.morph_advanced)?;
        if app.morph_advanced {
            ui.space(16.0);
            ui.push_sizing(Some(ui.div_width().min(340.0)), None);
            morph_strength = ui.slider(
                gpu,
                "settings-morph-strength",
                "Morph strength",
                app.morph_strength,
                ui::widgets::slider::Options {
                    min: 0,
                    max: 100,
                    step: 1,
                    suffix: "%",
                },
            )?;
            ui.space(16.0);
            morph_duration = ui.slider(
                gpu,
                "settings-morph-duration",
                "Morph duration",
                app.morph_duration_ms,
                ui::widgets::slider::Options {
                    min: 20,
                    max: 1000,
                    step: 1,
                    suffix: " ms",
                },
            )?;
            ui.space(16.0);
            label(ui, gpu, "Morph interpolation", 13, true, true, false)?;
            ui.space(10.0);
            ui.push_style(Style {
                font_size: 15,
                padding: 12.0,
                background: Some(ui.theme().input),
                border_color: Some(ui.theme().border),
                radius: ui.corner_radii().control,
                ..ui.style()
            });
            morph_curve = ui.dropdown(
                gpu,
                "settings-morph-curve",
                &[
                    "Linear",
                    "Quadratic ease out",
                    "Cubic ease out (default)",
                    "Smoothstep",
                ],
                Curve::ALL
                    .iter()
                    .position(|curve| *curve == app.morph_curve)
                    .unwrap_or(2),
            )?;
            ui.pop_style();
            ui.pop_sizing();
        }
        ui.div_end();
        change
    } else {
        None
    };

    ui.space(16.0);
    label(ui, gpu, "Animation interpolation", 13, true, true, false)?;
    ui.space(10.0);
    ui.push_style(Style {
        font_size: 15,
        padding: 12.0,
        background: Some(ui.theme().input),
        border_color: Some(ui.theme().border),
        radius: ui.corner_radii().control,
        ..ui.style()
    });
    ui.push_sizing(Some(ui.div_width().min(340.0)), None);
    let curve = ui.dropdown(
        gpu,
        "settings-animation-curve",
        &Curve::LABELS,
        Curve::ALL
            .iter()
            .position(|curve| *curve == app.animation_curve)
            .unwrap_or(1),
    )?;
    ui.pop_sizing();
    ui.pop_style();
    ui.space(24.0);
    let smooth = ui.checkbox(
        gpu,
        "settings-smooth-scrolling",
        "Smooth scrolling",
        app.smooth_scrolling,
    )?;
    ui.space(8.0);
    label(
        ui,
        gpu,
        "Interpolate wheel and touchpad scrolling in containers.",
        13,
        false,
        true,
        false,
    )?;
    ui.space(32.0);
    label(ui, gpu, "Zoom", 13, true, true, false)?;
    ui.space(16.0);
    ui.push_sizing(Some(ui.div_width().min(340.0)), None);
    let default_zoom = ui.slider(
        gpu,
        "settings-default-zoom",
        "Default zoom",
        app.default_zoom,
        ui::widgets::slider::Options {
            min: 50,
            max: 200,
            step: 10,
            suffix: "%",
        },
    )?;
    ui.space(8.0);
    label(
        ui,
        gpu,
        "The zoom applied when faith starts.",
        13,
        false,
        true,
        false,
    )?;
    ui.space(24.0);
    let zoom_step = ui.slider(
        gpu,
        "settings-zoom-step",
        "Zoom step",
        app.zoom_step,
        ui::widgets::slider::Options {
            min: 1,
            max: 50,
            step: 1,
            suffix: "%",
        },
    )?;
    ui.space(8.0);
    label(
        ui,
        gpu,
        "How much the zoom should change in percent per zoom adjustment.",
        13,
        false,
        true,
        false,
    )?;
    ui.pop_sizing();
    end_panel(ui);

    Ok(if let Some(enabled) = change {
        Action::Rounded(enabled)
    } else if let Some(enabled) = animations {
        Action::Animations(enabled)
    } else if let Some(enabled) = layout_morphing {
        Action::LayoutMorphing(enabled)
    } else if toggle_morph_advanced {
        Action::ToggleMorphAdvanced
    } else if let Some(strength) = morph_strength {
        Action::MorphStrength(strength)
    } else if let Some(duration) = morph_duration {
        Action::MorphDuration(duration)
    } else if let Some(index) = morph_curve {
        Action::MorphCurve(Curve::ALL[index])
    } else if let Some(enabled) = smooth {
        Action::SmoothScrolling(enabled)
    } else if let Some(step) = zoom_step {
        Action::ZoomStep(step)
    } else if let Some(percent) = default_zoom {
        Action::DefaultZoom(percent)
    } else if let Some(index) = curve {
        Action::AnimationCurve(Curve::ALL[index])
    } else {
        Action::None
    })
}

// The selected settings section and its controls.
fn settings_page(ui: &mut Framework, gpu: &mut Renderer, app: &App) -> anyhow::Result<Action> {
    if app.settings_item == SettingsItem::Misc {
        return misc_settings_page(ui, gpu, app);
    }

    let width = (ui.div_width() - 64.0).max(1.0);
    let height = (ui.div_height() - 72.0).max(1.0);
    ui.set_position(32.0, 36.0);
    ui.div_begin_id(
        "appearance-settings",
        DivOptions {
            width: Some(width),
            height: Some(height),
            scrollable: true,
        },
    )?;
    label(ui, gpu, "Appearance", 26, true, false, false)?;
    ui.space(28.0);

    label(ui, gpu, "Theme", 13, true, true, false)?;
    ui.space(10.0);
    let theme = app
        .themes
        .ids
        .iter()
        .position(|id| *id == app.theme)
        .unwrap_or(0);
    let theme_choice = if app.themes.names.is_empty() {
        None
    } else {
        ui.push_style(profile_input_style(ui));
        ui.push_sizing(Some(width.min(340.0)), None);
        let choice = ui.dropdown_shared(gpu, "settings-theme-dropdown", &app.themes.names, theme);
        ui.pop_sizing();
        ui.pop_style();
        choice?
    };

    ui.space(22.0);
    label(ui, gpu, "Font", 13, true, true, false)?;
    ui.space(10.0);
    ui.push_style(profile_input_style(ui));
    ui.push_sizing(Some(width.min(340.0)), None);
    let font_choice = ui.dropdown_shared(
        gpu,
        "settings-font-dropdown",
        &app.system_fonts.labels,
        app.system_fonts.selected,
    );
    ui.pop_sizing();
    ui.pop_style();
    let font_choice = font_choice?;
    if app.system_fonts.loading() {
        ui.space(8.0);
        label(ui, gpu, "Finding system fonts…", 12, false, true, false)?;
    }

    ui.div_end();
    Ok(if let Some(index) = font_choice {
        Action::Font(
            index
                .checked_sub(1)
                .map(|index| app.system_fonts.families[index].name.clone()),
        )
    } else if ui.dropdown_open("settings-font-dropdown") && app.system_fonts.needs_discovery() {
        Action::DiscoverFonts
    } else if let Some(index) = theme_choice {
        Action::Theme(app.themes.ids[index].clone())
    } else {
        Action::None
    })
}

// Arrange the navigation rail, item list and main content.
fn shell(ui: &mut Framework, gpu: &mut Renderer, app: &App, logo: Image) -> anyhow::Result<Action> {
    let width = ui.div_width();
    let height = ui.div_height();

    // The item list takes about a quarter of the available width.
    let rail = if app.collapsed {
        0.0
    } else {
        72.0
    };

    // Keep all lists aligned using the widest heading and its controls.
    ui.push_style(Style {
        font_size: 16,
        ..ui.style()
    });
    let title_size = ui.text_size("Conversations", Some(Font::Bold));
    ui.pop_style();
    let header_width = title_size?.0.ceil()
        + 44.0
        + if app.collapsed {
            118.0
        } else {
            80.0
        };

    let remaining = (width - rail).max(1.0);
    let list = (remaining / 4.0)
        .clamp(220.0_f32.max(header_width), 400.0_f32.max(header_width))
        .min((remaining - 200.0).max(100.0));
    let content = (remaining - list).max(1.0);

    let snapshot = &app.chat.snapshot;
    let mut action = Action::None;

    /* we keep the rail's identity while its width shrinks to zero */
    panel_named(ui, Some("navigation-rail"), [0.0, 0.0, rail, height], true)?;

    if ui.div_width() > 0.5 {
        ui.push_style(Style {
            enabled: !app.collapsed,
            ..ui.style()
        });
        action = navigation(ui, gpu, app, height)?;
        ui.pop_style();
    }

    end_panel(ui);
    panel_named(ui, Some("item-list"), [rail, 0.0, list, height], true)?;

    // Settings has its own list beside profiles and conversations.
    let list_action = if app.page == Page::Settings {
        settings_list(ui, gpu, app, height)?
    } else if let Page::Contacts(selected) = app.page {
        contacts_ui::list(ui, gpu, app, height, selected)?
    } else {
        item_list(ui, gpu, app, snapshot, height)?
    };

    if !matches!(list_action, Action::None) {
        action = list_action;
    }

    end_panel(ui);
    panel_named(
        ui,
        Some("pane-separator"),
        [rail + list, 0.0, 1.0, height],
        true,
    )?;
    ui.push_style(Style {
        background: Some(ui.theme().separator),
        ..ui.style()
    });

    ui.div_begin(DivOptions {
        width: Some(1.0),
        height: Some(height),
        ..Default::default()
    })?;

    ui.div_end();
    ui.pop_style();
    end_panel(ui);
    panel_named(
        ui,
        Some("content-pane"),
        [rail + list + 1.0, 0.0, content - 1.0, height],
        false,
    )?;

    let history_action = history_controls(ui, gpu, app)?;
    let pane_width = ui.div_width();
    let pane_height = ui.div_height();
    panel(
        ui,
        0.0,
        72.0,
        pane_width,
        (pane_height - 72.0).max(0.0),
        false,
    )?;

    let copied = app.copied_at > app.saved_at;
    let confirmation = if copied {
        app.copied_at
    } else {
        app.saved_at
    };
    let sliding = ui.slider_dragging();
    let opacity = confirmation
        .map(|at| settings::confirmation_opacity(at, app.animations, app.animation_curve))
        .unwrap_or(0.0);
    let error = !app.settings_error.is_empty() && (!copied || opacity <= 0.0);
    if !sliding
        && !error
        && let Some(deadline) = confirmation.and_then(|at| {
            settings::confirmation_deadline(at, std::time::Instant::now(), app.animations)
        })
    {
        ui.animate_until(deadline);
    }

    let show_clipboard = copied && !sliding && opacity > 0.0;
    let mut clipboard_position = None;

    // Render the current page in the main content pane.
    let content_action = match app.page {
        Page::Accounts => brand_page(ui, gpu, logo, app.profiles.is_empty())?,

        Page::Create | Page::CreateUsername => profile_creation_page(ui, gpu, app)?,
        Page::Conversations => {
            let (action, position) = chat_ui::conversation(ui, gpu, app, show_clipboard)?;
            clipboard_position = position;
            action
        }

        Page::Requests => chat_ui::requests(ui, gpu, app)?,
        Page::PendingInvites => chat_ui::pending_invites(ui, gpu, app)?,
        Page::CreateConversation => chat_ui::creation(ui, gpu, app)?,
        Page::Settings => settings_page(ui, gpu, app)?,
        Page::Contacts(selected) => contacts_ui::page(ui, gpu, app, selected)?,
    };

    if let Some(position) = clipboard_position {
        confirmation_text(
            ui,
            gpu,
            "Copied to system clipboard",
            position,
            pane_width - 40.0,
            opacity,
            false,
        )?;
    }

    end_panel(ui);
    if !matches!(content_action, Action::None) {
        action = content_action;
    }

    if !matches!(history_action, Action::None) {
        action = history_action;
    }

    if !app.error.is_empty() {
        ui.set_position(28.0, (height - 42.0).max(0.0));
        ui.push_style(Style {
            font_size: 13,
            color: ui.theme().error,
            ..ui.style()
        });

        ui.push_sizing(Some((content - 56.0).max(1.0)), None);
        ui.text(gpu, &app.error, None)?;
        ui.pop_sizing();
        ui.pop_style();
    }

    if clipboard_position.is_none() && ((!sliding && opacity > 0.0) || error) {
        let feedback = if error {
            &app.settings_error
        } else if copied {
            "Copied to system clipboard"
        } else {
            "Settings saved"
        };
        confirmation_text(
            ui,
            gpu,
            feedback,
            [28.0, (height - 68.0).max(0.0)],
            content - 56.0,
            opacity,
            error,
        )?;
    }

    end_panel(ui);
    Ok(action)
}

fn confirmation_text(
    ui: &mut Framework,
    gpu: &mut Renderer,
    text: &str,
    [x, y]: [f32; 2],
    width: f32,
    opacity: f32,
    error: bool,
) -> anyhow::Result<()> {
    let mut color = if error {
        ui.theme().error
    } else {
        ui.theme().muted
    };
    color.a *= if error {
        1.0
    } else {
        opacity
    };
    ui.set_position(x, y);
    ui.push_style(Style {
        font_size: 12,
        color,
        ..ui.style()
    });
    ui.push_sizing(Some(width.max(1.0)), None);
    let result = ui.text(gpu, text, None);
    ui.pop_sizing();
    ui.pop_style();
    result
}

// Start the window and run the blocking UI loop.
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(if cfg!(debug_assertions) {
            tracing::Level::INFO
        } else {
            tracing::Level::WARN
        })
        .init();

    // Read startup options and the local TLS configuration.
    let matches = cli::Cli::command().get_matches();
    let explicit_theme =
        matches.value_source("theme") == Some(clap::parser::ValueSource::CommandLine);
    let mut cli = cli::Cli::from_arg_matches(&matches)?;
    cli.resolve_local_tls();
    let config = WindowConfig {
        backend: cli.platform,
        title: "faith.".into(),
        width: cli.width.max(800),
        height: cli.height.max(540),
    };

    // Create the window and select a graphics backend.
    let mut platform = Platform::new(&config)?;
    let mut renderer = Renderer::new(cli.graphics.into(), &platform)?;
    let assets = assets::directory()?;

    // Register text and icon fonts; glyphs are loaded when needed.
    let mut ui = Framework::new(
        &mut renderer,
        &platform,
        Fonts {
            regular: assets.join("NotoSans-Regular.ttf"),
            bold: assets.join("NotoSans-Bold.ttf"),
            italic: Some(assets.join("NotoSans-Italic.ttf")),
            icons: assets.join("nerd-fonts/SymbolsNerdFont-Regular.ttf"),
            emoji: assets.join("NotoColorEmoji.ttf"),
            fallbacks: ["NotoSansArabic-Regular.ttf", "NotoSansHebrew-Regular.ttf"]
                .iter()
                .map(|f| assets.join(f))
                .collect(),
        },
    )?;
    let root = profiles::root()?;
    let settings_path = root.parent().unwrap().join("settings.json");
    let (mut preferences, mut settings_error) = match settings::Settings::load(&settings_path) {
        Ok(settings) => (settings, String::new()),
        Err(error) => (settings::Settings::default(), format!("{error:#}")),
    };

    if explicit_theme {
        preferences.theme = cli.theme.clone();
    }

    let themes = Catalog::open(root.parent().unwrap().join("themes"))?;
    preferences.theme = theme::canonical_id(&preferences.theme).to_owned();
    let colors = match themes.load(&preferences.theme) {
        Ok(colors) => colors,
        Err(error) => {
            settings_error = format!("{error:#}");
            preferences.theme = theme::DEFAULT_ID.to_owned();
            themes.load(theme::DEFAULT_ID).unwrap_or_default()
        }
    };
    if let Err(error) = ui.set_system_font(preferences.font.as_ref()) {
        settings_error = format!("Could not restore the font: {error:#}");
        preferences.font = None;
    }

    ui.set_theme(colors);
    ui.set_rounded_corners(preferences.rounded_corners);
    ui.set_animations(preferences.animations, preferences.animation_curve);
    ui.set_layout_morphing(preferences.layout_morphing);
    ui.set_morph_parameters(
        preferences.morph_duration_ms,
        preferences.morph_curve,
        preferences.morph_strength,
    );
    ui.set_smooth_scrolling(preferences.smooth_scrolling);
    ui.set_zoom_step(preferences.zoom_step);
    ui.set_startup_zoom(preferences.default_zoom);

    // Discover the accounts stored on this machine.
    let profiles = profiles::discover(&root)?;
    let mut app = App {
        contacts: contacts::Book::open(&root)?,
        contact_mode: contacts_ui::Mode::View,
        contact_delete_in_list: None,
        contact_usernames: Default::default(),
        contact_conversations: Vec::new(),
        contact_links: Vec::new(),
        root,
        profiles,
        profile_form: ProfileForm::default(),
        selected: None,
        session: None,
        chat: chat_ui::State::default(),
        page: Page::Accounts,
        tab_locations: [None; 4],
        back_history: Vec::new(),
        forward_history: Vec::new(),
        error: String::new(),
        collapsed: false,
        list_offset: 0,
        conversation: None,
        theme: preferences.theme,
        themes,
        system_fonts: system_fonts::Catalog::new(preferences.font.as_ref()),
        font: preferences.font,
        settings_item: SettingsItem::Appearance,
        rounded: preferences.rounded_corners,
        animations: preferences.animations,
        layout_morphing: preferences.layout_morphing,
        morph_duration_ms: preferences.morph_duration_ms,
        morph_curve: preferences.morph_curve,
        morph_strength: preferences.morph_strength,
        morph_advanced: false,
        animation_curve: preferences.animation_curve,
        smooth_scrolling: preferences.smooth_scrolling,
        zoom_step: preferences.zoom_step,
        default_zoom: preferences.default_zoom,
        settings: settings::Store::start(settings_path, platform.waker())?,
        saved_at: None,
        copied_at: None,
        settings_error,
    };

    for path in app
        .profiles
        .iter()
        .map(|profile| &profile.path)
        .chain(cli.profile.iter())
    {
        if let Err(error) = app.contacts.import(path) {
            tracing::warn!("Could not import contacts from {}: {error}", path.display());
        }
    }

    app.load_contact_usernames(cli.profile.as_deref());

    // Use an explicit profile, or automatically select the only account.
    if let Some(path) = &cli.profile {
        app.login(
            Profile {
                alias: path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Account")
                    .to_owned(),
                path: path.clone(),
            },
            &cli,
            &platform,
        );
    } else if app.profiles.len() == 1 {
        app.login(app.profiles[0].clone(), &cli, &platform);
    } else if !app.profiles.is_empty() {
        app.page = Page::Accounts;
    }

    platform.request_redraw()?;
    let mut logo = None;
    let mut quickshell = quickshell::Shell::default();
    let mut popup = ui::widgets::popup::State::default();
    let mut popup_menu = PopupMenu::Header;

    // Wait for input or a client notification before drawing again.
    loop {
        let mut events = platform.poll_events()?;
        app.refresh_chat(&mut ui);
        if app.system_fonts.poll(app.font.as_ref()) {
            quickshell.finish_font_picker(&app);
        }

        events.retain(|event| {
            let requested = matches!(event, WindowEvent::QuickShellRequested)
                || (!quickshell.is_open()
                    && !ui.space_reserved()
                    && matches!(event, WindowEvent::TextInput(text) if text == " "));
            if requested && !quickshell.is_open() {
                popup.close();
                quickshell.open(&app, ui.zoom_percent(), &mut ui);
            }

            !requested
        });

        let modal_frame = quickshell.is_open();
        let popup_frame = popup.is_open();
        ui.set_modal_input(modal_frame || popup.is_open());
        if modal_frame && events.is_empty() {
            events.push(WindowEvent::RedrawRequested);
        }

        // Handle window shutdown and resizing first.
        for event in &events {
            match *event {
                WindowEvent::CloseRequested => return Ok(()),
                WindowEvent::Resized { width, height } if width > 0 && height > 0 => {
                    renderer.resize(width, height)
                }

                _ => {}
            }
        }

        // Show confirmation only after the settings have reached disk.
        if let Some(result) = app.settings.poll() {
            match result {
                Ok(saved) => {
                    app.saved_at = Some(saved);
                    app.settings_error.clear();
                }

                Err(error) => app.settings_error = error,
            }
        }

        let (w, h) = platform.size();

        if w == 0 || h == 0 || events.is_empty() {
            continue;
        }

        // Begin a new UI pass with the active theme.
        ui.update_animations();
        renderer.clear_color(ui.theme().background);
        renderer.begin_frame()?;
        ui.begin(&platform, &events);

        let first_frame = logo.is_none();

        // Upload the logo once and reuse its image handle.
        let logo = match logo {
            Some(image) => image,
            None => {
                let image = ui.load_image(&mut renderer, &assets.join("logo.png"))?;
                logo = Some(image);
                image
            }
        };

        // Draw the shell, then apply any action returned by the widgets.
        let mut action = shell(&mut ui, &mut renderer, &app, logo)?;
        if quickshell.is_visible() {
            let shell_action = quickshell.render(&mut ui, &mut renderer, &app)?;

            if modal_frame {
                action = shell_action.unwrap_or(Action::None);
            }
        }
        if popup.is_visible() {
            use ui::widgets::popup::Item;
            match &popup_menu {
                PopupMenu::Header => {
                    let mut items = vec![
                        Item {
                            icon: "",
                            label: "Open Quickshell",
                        },
                        Item {
                            icon: "\u{ea76}",
                            label: "Quit faith",
                        },
                    ];
                    if app.page.is_chat() && app.selected.is_some() {
                        items.insert(
                            0,
                            Item {
                                icon: NEW_CHAT,
                                label: "New conversation",
                            },
                        );
                    }

                    if let Some(index) =
                        ui.popup(&mut renderer, "header-menu", &mut popup, &items)?
                    {
                        action = match (items.len(), index) {
                            (3, 0) => Action::NewConversation(app.selected.clone().unwrap()),
                            (3, 1) | (2, 0) => Action::OpenQuickshell,
                            _ => Action::Quit,
                        };
                    }
                }

                PopupMenu::Row(target) => {
                    let (glyph, label, command) = match target {
                        RowMenu::Profile(profile) => (
                            contacts::DELETE,
                            "Delete profile",
                            Action::ConfirmDeleteProfile(profile.clone()),
                        ),
                        RowMenu::Contact(id) => (
                            contacts::DELETE,
                            "Delete contact",
                            Action::ConfirmDeleteContactInList(*id),
                        ),
                        RowMenu::Conversation { profile, id } => (
                            contacts::EDIT,
                            "Rename conversation",
                            Action::BeginConversationRename {
                                profile: profile.clone(),
                                id: *id,
                            },
                        ),
                    };
                    let items = [Item { icon: glyph, label }];
                    if ui
                        .popup(&mut renderer, "row-menu", &mut popup, &items)?
                        .is_some()
                    {
                        action = command;
                    }
                }

                PopupMenu::ConfirmDeleteProfile(profile) => {
                    let label = format!("Permanently delete {}", profile.alias);
                    let items = [
                        Item {
                            icon: "",
                            label: "Cancel",
                        },
                        Item {
                            icon: contacts::DELETE,
                            label: &label,
                        },
                    ];
                    if ui.popup(&mut renderer, "delete-profile", &mut popup, &items)? == Some(1) {
                        action = Action::DeleteProfile(profile.clone());
                    }
                }
            }
        }

        ui.end(&mut renderer)?;
        platform.set_cursor(ui.cursor_icon())?;
        renderer.end_frame()?;

        if first_frame {
            if let Some(path) = &cli.ready_file {
                std::fs::write(path, b"ready\n")?;
            }
            tracing::info!("First UI frame rendered");
        }

        if !quickshell.is_open() && !popup.is_open() {
            ui.set_modal_input(false);
        }

        let mut clipboard = ui.take_clipboard_actions();
        match action {
            Action::OpenMenu | Action::OpenRowMenu(..) | Action::ConfirmDeleteProfile(_) => {
                ui.set_modal_input(false);
                let at = ui
                    .pointer_position()
                    .unwrap_or(CursorPosition { x: 24.0, y: 56.0 });
                let (menu, at) = match action {
                    Action::OpenRowMenu(target, at) => (PopupMenu::Row(target), at),
                    Action::ConfirmDeleteProfile(profile) => {
                        (PopupMenu::ConfirmDeleteProfile(profile), at)
                    }
                    _ => (PopupMenu::Header, at),
                };
                if matches!(menu, PopupMenu::Row(_)) {
                    popup.open_context(at, std::time::Instant::now());
                } else {
                    popup.open(at);
                }

                popup_menu = menu;
                platform.request_redraw()?;
                continue;
            }
            Action::OpenQuickshell | Action::OpenContactPicker => {
                ui.set_modal_input(false);
                if matches!(action, Action::OpenContactPicker) {
                    quickshell.open_contact_picker(&app, &mut ui);
                } else {
                    quickshell.open(&app, ui.zoom_percent(), &mut ui);
                }

                platform.request_redraw()?;
                continue;
            }
            Action::OpenFontPicker => {
                if let Err(error) = app.system_fonts.start(platform.waker()) {
                    app.error = format!("Could not discover system fonts: {error:#}");
                } else {
                    quickshell.open_font_picker(&app, &mut ui);
                }

                platform.request_redraw()?;
                continue;
            }

            Action::OpenConversationContacts => {
                ui.set_modal_input(false);
                quickshell.open_conversation_contacts(&app, &mut ui);
                platform.request_redraw()?;
                continue;
            }

            Action::PromptSaveConversationContact {
                profile,
                id,
                member,
            } => {
                ui.set_modal_input(false);
                match app.check_conversation_contact(&profile, id, member) {
                    Ok(()) => quickshell.prompt_save_contact(
                        &mut ui,
                        profile,
                        id,
                        member,
                        app.contact_usernames.get(&member).map(String::as_str),
                    ),
                    Err(error) => app.error = error.to_string(),
                }
                platform.request_redraw()?;
                continue;
            }

            Action::Quit => return Ok(()),
            _ => {}
        }
        if (modal_frame && !quickshell.is_open()) || (popup_frame && !popup.is_open()) {
            platform.request_redraw()?;
        }

        if let Err(error) = app.act(action, &cli, &platform, &mut ui, &mut clipboard) {
            app.error = error.to_string();
            platform.request_redraw()?;
        }

        for action in clipboard {
            let result = match action {
                ui::framework::ClipboardAction::Copy(text) => {
                    platform.copy_text(text).and_then(|()| {
                        app.copied_at = Some(std::time::Instant::now());
                        platform.request_redraw()
                    })
                }
                ui::framework::ClipboardAction::Paste(target) => platform.paste_text(target),
            };

            if let Err(error) = result {
                app.error = format!("Clipboard: {error}");
                platform.request_redraw()?;
            }
        }
    }
}
