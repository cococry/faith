use std::path::PathBuf;

use super::rank::Text;
use crate::{Action, App, Page, Profile, SettingsItem, ui::animation::Curve};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Navigation,
    Profile,
    Conversation,
    Settings,
    Theme,
}

pub struct Context {
    pub kind: Kind,
    pub profile: Option<PathBuf>,
}

impl Context {
    pub fn from_app(app: &App) -> Self {
        Self {
            kind: match app.page {
                Page::Accounts | Page::Create => Kind::Profile,
                Page::Conversations | Page::CreateConversation => Kind::Conversation,
                Page::Settings => Kind::Settings,
            },
            profile: app.selected.as_ref().map(|profile| profile.path.clone()),
        }
    }
}

pub struct Entry {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub kind: Kind,
    pub primary: Text,
    pub terms: Vec<Text>,
    pub command: Action,
    pub children: Option<Vec<Entry>>,
    pub priority: i32,
    pub owner: Option<PathBuf>,
}

impl Entry {
    fn new(id: &str, label: &str, kind: Kind, command: Action) -> Self {
        let setting = match &command {
            Action::Rounded(_) => Some((SettingsItem::Misc, "settings-rounded-corners")),
            Action::Animations(_) => Some((SettingsItem::Misc, "settings-animations")),
            Action::LayoutMorphing(_) => Some((SettingsItem::Misc, "settings-layout-morphing")),
            Action::SmoothScrolling(_) => Some((SettingsItem::Misc, "settings-smooth-scrolling")),
            Action::ZoomStep(_) => Some((SettingsItem::Misc, "settings-zoom-step")),
            Action::DefaultZoom(_) => Some((SettingsItem::Misc, "settings-default-zoom")),
            _ => None,
        };
        let command = if let Some((section, control)) = setting {
            Action::FocusSetting {
                action: Box::new(command),
                section,
                control,
            }
        } else {
            command
        };
        Self {
            id: id.into(),
            label: label.into(),
            detail: String::new(),
            kind,
            primary: Text::new(label),
            terms: Vec::new(),
            command,
            children: None,
            priority: 100,
            owner: None,
        }
    }

    fn search(mut self, primary: &str, aliases: &[&str]) -> Self {
        self.terms
            .push(std::mem::replace(&mut self.primary, Text::new(primary)));
        self.terms
            .extend(aliases.iter().map(|alias| Text::new(alias)));
        self
    }

    fn detail(mut self, detail: &str) -> Self {
        self.detail = detail.into();
        self
    }

    fn list(mut self, children: impl IntoIterator<Item = Entry>) -> Self {
        self.children = Some(children.into_iter().collect());
        self
    }

    fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn profile(profile: &Profile) -> Self {
        let mut entry = Self::new(
            &format!("profile:{}", profile.path.display()),
            &format!("Open profile {}", profile.alias),
            Kind::Profile,
            Action::OpenProfile(profile.clone()),
        )
        .search(&profile.alias, &["profile account switch login sign in"])
        .detail(&format!("Profile · {}", profile.alias))
        .priority(160);
        entry.owner = Some(profile.path.clone());
        entry
    }

    /* the destination carries its owner, so a chat never opens under the wrong profile */
    pub fn conversation(profile: &Profile, id: faith_client::Id, name: &str) -> Self {
        let mut entry = Self::new(
            &format!("conversation:{}:{id}", profile.path.display()),
            name,
            Kind::Conversation,
            Action::OpenConversation {
                profile: profile.clone(),
                id,
            },
        )
        .search(name, &["chat conversation message talk open"])
        .detail(&format!("Conversation · {}", profile.alias))
        .priority(200);
        entry.terms.push(Text::new(&profile.alias));
        entry.owner = Some(profile.path.clone());
        entry
    }

    pub fn context_bonus(&self, context: &Context) -> i32 {
        let category = if self.kind == context.kind {
            300
        } else if context.kind == Kind::Settings && self.kind == Kind::Theme {
            180
        } else if context.kind == Kind::Settings && self.kind == Kind::Conversation {
            60
        } else {
            0
        };
        category
            + if self.owner.is_some() && self.owner == context.profile {
                35
            } else {
                0
            }
    }

    pub fn icon(&self) -> &'static str {
        if matches!(self.command, Action::NewConversation(_)) {
            return crate::NEW_CHAT;
        }

        match self.kind {
            Kind::Profile => crate::ACCOUNTS,
            Kind::Conversation => crate::CHATS,
            Kind::Settings => crate::SETTINGS,
            Kind::Theme => crate::THEME,
            Kind::Navigation => crate::NEXT,
        }
    }
}

pub fn build(app: &App, zoom: u32) -> Vec<Entry> {
    let mut entries = vec![
        Entry::new(
            "open-menu",
            "Open application menu",
            Kind::Navigation,
            Action::OpenMenu,
        )
        .search("application menu", &["ellipsis more popup commands"])
        .detail("Application"),
        Entry::new(
            "open-quickshell",
            "Open Quickshell",
            Kind::Navigation,
            Action::OpenQuickshell,
        )
        .search("quickshell", &["finder commands search space"])
        .detail("Application"),
        Entry::new("quit-faith", "Quit faith", Kind::Navigation, Action::Quit)
            .search("quit faith", &["exit close application"])
            .detail("Application"),
        Entry::new(
            "profiles",
            "Go to profiles",
            Kind::Navigation,
            Action::Choose,
        )
        .search(
            "profiles",
            &["accounts account list switch profile home branding"],
        )
        .detail("Navigation")
        .priority(180),
        Entry::new(
            "create-profile",
            "Create profile",
            Kind::Profile,
            Action::New,
        )
        .search(
            "create profile",
            &["create new account add profile new profile register"],
        )
        .detail("Open the profile creation form")
        .priority(if app.profiles.is_empty() {
            450
        } else {
            160
        }),
        Entry::new(
            "settings",
            "Go to settings",
            Kind::Navigation,
            Action::Settings,
        )
        .search("settings", &["preferences options configuration"])
        .detail("Navigation")
        .priority(190),
        Entry::new(
            "settings-theme",
            "Theme settings",
            Kind::Settings,
            Action::OpenSettings {
                section: SettingsItem::Theme,
                control: None,
            },
        )
        .search("theme", &["appearance colors colours settings preferences"])
        .detail("Settings · Appearance")
        .priority(180),
        Entry::new(
            "settings-misc",
            "Misc settings",
            Kind::Settings,
            Action::OpenSettings {
                section: SettingsItem::Misc,
                control: None,
            },
        )
        .search(
            "misc",
            &["miscellaneous interface settings preferences options"],
        )
        .detail("Settings · Interface")
        .priority(170),
        Entry::new(
            "zoom-step",
            "Adjust zoom step",
            Kind::Settings,
            Action::OpenSettings {
                section: SettingsItem::Misc,
                control: Some("settings-zoom-step"),
            },
        )
        .search("zoom step", &["zoom increment scaling amount settings"])
        .detail(&format!(
            "Settings · Currently {} percentage points",
            app.zoom_step
        )),
        Entry::new(
            "default-zoom",
            "Adjust default zoom",
            Kind::Settings,
            Action::OpenSettings {
                section: SettingsItem::Misc,
                control: Some("settings-default-zoom"),
            },
        )
        .search(
            "default zoom",
            &["startup zoom scale initial size settings"],
        )
        .detail(&format!("Settings · Currently {}%", app.default_zoom)),
        Entry::new(
            "sidebar",
            if app.collapsed {
                "Expand sidebar"
            } else {
                "Collapse sidebar"
            },
            Kind::Navigation,
            Action::ToggleMenu,
        )
        .search("sidebar", &["navigation menu rail toggle collapse expand"])
        .detail("Navigation"),
    ];
    for (id, title, current, on, off) in [
        (
            "rounded",
            "rounded corners",
            app.rounded,
            Action::Rounded(true),
            Action::Rounded(false),
        ),
        (
            "animations",
            "UI animations",
            app.animations,
            Action::Animations(true),
            Action::Animations(false),
        ),
        (
            "smooth-scroll",
            "smooth scrolling",
            app.smooth_scrolling,
            Action::SmoothScrolling(true),
            Action::SmoothScrolling(false),
        ),
    ] {
        entries.push(
            Entry::new(
                id,
                &format!(
                    "{} {title}",
                    if current {
                        "Disable"
                    } else {
                        "Enable"
                    }
                ),
                Kind::Settings,
                if current {
                    off
                } else {
                    on
                },
            )
            .search(
                &format!(
                    "{} {title}",
                    if current {
                        "disable"
                    } else {
                        "enable"
                    }
                ),
                &[
                    "toggle interface settings",
                    if id == "animations" {
                        "layout motion morph warp transitions fade quickshell selection interpolation"
                    } else {
                        ""
                    },
                    if current {
                        "turn off"
                    } else {
                        "turn on"
                    },
                ],
            )
            .detail("Settings · Misc"),
        );
    }

    if app.animations {
        entries.push(
            Entry::new(
                "layout-morphing",
                if app.layout_morphing {
                    "Disable layout morphing animations"
                } else {
                    "Enable layout morphing animations"
                },
                Kind::Settings,
                Action::LayoutMorphing(!app.layout_morphing),
            )
            .search(
                "layout morphing animations",
                &["warp geometry movement resize position animation interpolation toggle"],
            )
            .detail("Settings · Misc · UI animations"),
        );
    }

    let mut chats = Vec::new();
    for profile in app.profiles.iter().filter(|profile| {
        app.selected
            .as_ref()
            .is_none_or(|active| active.path != profile.path)
    }) {
        entries.push(Entry::profile(profile));
        match crate::client::conversation_titles(&profile.path) {
            Ok(titles) => chats.extend(
                titles
                    .into_iter()
                    .map(|(id, title)| Entry::conversation(profile, id, &title)),
            ),
            Err(error) => tracing::warn!(
                "Could not list conversations for {}: {error}",
                profile.alias
            ),
        }
    }

    if let Some(profile) = &app.selected {
        entries.push(Entry::profile(profile));
        let snapshot = &app.chat.snapshot;
        if snapshot.auth.is_some() {
            chats.extend(
                snapshot
                    .conversations
                    .iter()
                    .map(|chat| Entry::conversation(profile, chat.id, &chat.title)),
            );
        } else if let Ok(titles) = crate::client::conversation_titles(&profile.path) {
            chats.extend(
                titles
                    .into_iter()
                    .map(|(id, title)| Entry::conversation(profile, id, &title)),
            );
        }

        entries.push(
            Entry::new(
                "conversations",
                "Go to conversations",
                Kind::Navigation,
                Action::Chats,
            )
            .search("conversations", &["chats messages current profile inbox"])
            .detail(&format!("Current profile · {}", profile.alias))
            .priority(210),
        );
        entries.push(
            Entry::new(
                "reconnect",
                "Reconnect current profile",
                Kind::Conversation,
                Action::Retry,
            )
            .search("reconnect", &["retry connection connect server"])
            .detail(&profile.alias),
        );
        let snapshot = &app.chat.snapshot;
        for (available, id, title, aliases, action) in [
            (
                app.session.is_some(),
                "new-conversation",
                "New conversation",
                "start chat create group",
                Action::NewConversation(profile.clone()),
            ),
            (
                snapshot.auth.is_some(),
                "copy-auth-id",
                "Copy Auth ID",
                "copy user id identity",
                Action::CopyAuthId(profile.clone()),
            ),
            (
                snapshot.pending && !snapshot.delivering && snapshot.authorized,
                "retry-delivery",
                "Retry delivery",
                "retry message outbox",
                Action::RetryDelivery(profile.clone()),
            ),
            (
                snapshot.pending && !snapshot.delivering,
                "discard-delivery",
                "Discard pending delivery",
                "discard message outbox",
                Action::DiscardDelivery(profile.clone()),
            ),
        ] {
            if available {
                let mut entry = Entry::new(id, title, Kind::Conversation, action)
                    .search(title, &[aliases])
                    .detail(&profile.alias);
                entry.owner = Some(profile.path.clone());
                entries.push(entry);
            }
        }

        if app.page == Page::CreateConversation && app.chat.editing_creation() && !app.chat.naming {
            if app.chat.partners.len() < faith_client::MAX_MEMBERS - 1 {
                entries.push(
                    Entry::new(
                        "add-chat-partner",
                        "Add chat partner",
                        Kind::Conversation,
                        Action::FocusChatPartner,
                    )
                    .search("add chat partner", &["add recipient member participant"]),
                );
            }

            if !app.chat.partners.is_empty() {
                entries.push(
                    Entry::new(
                        "remove-chat-partner",
                        "Remove chat partner",
                        Kind::Conversation,
                        Action::None,
                    )
                    .search(
                        "remove chat partner",
                        &["remove recipient member participant"],
                    )
                    .list(
                        app.chat
                            .partners
                            .iter()
                            .enumerate()
                            .map(|(index, (key, _))| {
                                Entry::new(
                                    &format!("remove-chat-partner:{key}"),
                                    &format!("Remove partner {}", index + 1),
                                    Kind::Conversation,
                                    Action::RemoveChatPartner(index),
                                )
                            }),
                    ),
                );
            }

            entries.push(Entry::new(
                "cancel-new-conversation",
                "Cancel new conversation",
                Kind::Conversation,
                Action::CancelConversation,
            ));
        }

        if !snapshot.conversations.is_empty() {
            let children = snapshot.conversations.iter().map(|chat| {
                let mut entry = Entry::new(
                    &format!("name-conversation:{}:{}", profile.path.display(), chat.id),
                    &chat.title,
                    Kind::Conversation,
                    Action::PromptConversationName {
                        profile: profile.clone(),
                        id: chat.id,
                    },
                )
                .search(&chat.title, &["name rename title"])
                .detail(&profile.alias);
                entry.owner = Some(profile.path.clone());
                entry
            });
            entries.push(
                Entry::new(
                    "name-conversation",
                    "Name a conversation",
                    Kind::Conversation,
                    Action::None,
                )
                .search("name a conversation", &["rename chat title group"])
                .detail(&profile.alias)
                .list(children),
            );
        }
    }

    if !chats.is_empty() {
        entries.push(
            Entry::new(
                "profile-conversations",
                "Open conversation",
                Kind::Conversation,
                Action::None,
            )
            .search("open conversation", &["find chat group messages"])
            .list(chats),
        );
    }

    entries.push(
        Entry::new("select-theme", "Theme selector", Kind::Theme, Action::None)
            .search(
                "theme",
                &["select theme theme selector choose theme appearance colors colours switch"],
            )
            .detail("Browse installed themes")
            .list(
                app.themes
                    .ids
                    .iter()
                    .zip(&app.themes.names)
                    .map(|(id, name)| {
                        Entry::new(
                            &format!("theme:{id}"),
                            name,
                            Kind::Theme,
                            Action::Theme(id.clone()),
                        )
                        .search(name, &["theme appearance colors colours switch"])
                        .detail(if id == &app.theme {
                            "Current theme"
                        } else {
                            "Apply theme"
                        })
                    }),
            ),
    );

    entries.push(
        Entry::new(
            "select-animation-curve",
            "Animation interpolation selector",
            Kind::Settings,
            Action::None,
        )
        .search(
            "animation interpolation",
            &["select animation interpolation function easing curve selector choose"],
        )
        .detail("Choose the animation interpolation function")
        .list(Curve::ALL.iter().enumerate().map(|(index, curve)| {
            Entry::new(
                &format!("curve:{index}"),
                Curve::LABELS[index],
                Kind::Settings,
                Action::AnimationCurve(*curve),
            )
            .search(
                Curve::LABELS[index],
                &["animation easing interpolation curve function"],
            )
            .detail(if *curve == app.animation_curve {
                "Current interpolation function"
            } else {
                "Apply interpolation function"
            })
        })),
    );

    if !app.back_history.is_empty() || app.page != Page::Accounts {
        entries.push(
            Entry::new("back", "Go back", Kind::Navigation, Action::Back)
                .search("back", &["previous history return"])
                .detail("Navigation history"),
        );
    }

    if !app.forward_history.is_empty() {
        entries.push(
            Entry::new("forward", "Go forward", Kind::Navigation, Action::Forward)
                .search("forward", &["next history"])
                .detail("Navigation history"),
        );
    }

    if zoom != 100 {
        entries.push(
            Entry::new(
                "reset-zoom",
                "Reset zoom to 100%",
                Kind::Navigation,
                Action::ResetZoom,
            )
            .search("reset zoom", &["zoom normal original size 100 scale"])
            .detail(&format!("Currently {zoom}%")),
        );
    }

    entries
}
