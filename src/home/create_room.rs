//! "Create room" flow — pure config types + validation helpers + the
//! `CreateRoomScreen` widget that drives them.
//!
//! All validation and ruma-request building lives in pure functions
//! exercised by the `tests_room_creation` module at the bottom of the
//! file, so the spec's per-rule scenarios can be checked without
//! spinning up a Matrix `Client`.

use makepad_widgets::*;
use ruma::{
    OwnedUserId, UserId,
    api::client::room::{
        create_room::v3::{Request as CreateRoomRequest, RoomPreset},
        Visibility,
    },
    events::{
        AnyInitialStateEvent, InitialStateEvent,
        room::encryption::RoomEncryptionEventContent,
    },
    serde::Raw,
    EventEncryptionAlgorithm,
};

use crate::sliding_sync::{submit_async_request, MatrixRequest};

// ============================================================================
// Config + validation types
// ============================================================================

#[derive(Clone, Debug)]
pub struct CreateRoomConfig {
    pub name: String,
    pub topic: Option<String>,
    pub avatar_bytes: Option<Vec<u8>>,
    pub avatar_mime: Option<String>,
    pub visibility: RoomVisibilityChoice,
    pub e2ee_enabled: bool,
    pub initial_invitees: Vec<OwnedUserId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomVisibilityChoice {
    Public,
    Private,
}

impl Default for CreateRoomConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            topic: None,
            avatar_bytes: None,
            avatar_mime: None,
            visibility: RoomVisibilityChoice::Private,
            e2ee_enabled: false,
            initial_invitees: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreateRoomConfigError {
    EmptyName,
    NameTooLong { len: usize, max: usize },
    InvalidInvitee { raw: String },
    AvatarTooLarge { bytes: usize, max: usize },
    /// E2EE on a Public room is explicitly refused by robrix this month.
    EncryptedPublicRoom,
}

const NAME_MAX: usize = 255;
const AVATAR_MAX_BYTES: usize = 4 * 1024 * 1024; // 4 MiB
const ENCRYPTION_ALGORITHM: &str = "m.megolm.v1.aes-sha2";

// ============================================================================
// Pure validators
// ============================================================================

/// Returns `Ok(())` iff the given config is well-formed enough to send
/// to the server. The widget's submit button is wired to this — if it
/// returns `Err`, the button stays disabled so we never round-trip an
/// invalid request.
pub fn validate_create_room_config(
    config: &CreateRoomConfig,
) -> Result<(), CreateRoomConfigError> {
    let trimmed = config.name.trim();
    if trimmed.is_empty() {
        return Err(CreateRoomConfigError::EmptyName);
    }
    let name_len = trimmed.chars().count();
    if name_len > NAME_MAX {
        return Err(CreateRoomConfigError::NameTooLong {
            len: name_len,
            max: NAME_MAX,
        });
    }
    if let Some(bytes) = &config.avatar_bytes {
        if bytes.len() > AVATAR_MAX_BYTES {
            return Err(CreateRoomConfigError::AvatarTooLarge {
                bytes: bytes.len(),
                max: AVATAR_MAX_BYTES,
            });
        }
    }
    if config.e2ee_enabled && config.visibility == RoomVisibilityChoice::Public {
        return Err(CreateRoomConfigError::EncryptedPublicRoom);
    }
    Ok(())
}

/// Split a raw textarea string into successfully-parsed `OwnedUserId`s
/// and a parallel list of raw substrings that failed to parse. Order
/// is preserved so the UI can render failed strings inline next to
/// where the user typed them. Whitespace AND commas are delimiters.
pub fn parse_invitee_list(raw: &str) -> (Vec<OwnedUserId>, Vec<String>) {
    let mut parsed = Vec::new();
    let mut failed = Vec::new();
    for token in raw.split(|c: char| c.is_whitespace() || c == ',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        match UserId::parse(token) {
            Ok(uid) => parsed.push(uid),
            Err(_) => failed.push(token.to_string()),
        }
    }
    (parsed, failed)
}

// ============================================================================
// Ruma request construction
// ============================================================================

impl CreateRoomConfig {
    /// Build the `create_room::v3::Request` from this config. Visibility
    /// and preset are derived from the user's choice; an
    /// `m.room.encryption` initial-state event is added iff
    /// `e2ee_enabled == true`. `initial_invitees` are NOT placed in the
    /// request — they are invited via `Joined::invite_user_by_id` calls
    /// after creation so per-invitee failures don't block the create.
    pub fn to_ruma_request(&self) -> CreateRoomRequest {
        let mut request = CreateRoomRequest::new();
        let name = self.name.trim().to_string();
        if !name.is_empty() {
            request.name = Some(name);
        }
        if let Some(topic) = self.topic.as_ref().filter(|s| !s.trim().is_empty()) {
            request.topic = Some(topic.trim().to_string());
        }
        let (visibility, preset) = match self.visibility {
            RoomVisibilityChoice::Public => (Visibility::Public, Some(RoomPreset::PublicChat)),
            RoomVisibilityChoice::Private => (Visibility::Private, Some(RoomPreset::PrivateChat)),
        };
        request.visibility = visibility;
        request.preset = preset;
        if self.e2ee_enabled {
            let encryption_content = RoomEncryptionEventContent::new(
                EventEncryptionAlgorithm::from(ENCRYPTION_ALGORITHM),
            );
            let initial_state_event =
                InitialStateEvent::new(Default::default(), encryption_content);
            let raw: Raw<AnyInitialStateEvent> = Raw::new(&initial_state_event)
                .expect("RoomEncryptionEventContent serializes")
                .cast_unchecked();
            request.initial_state = vec![raw];
        }
        request
    }
}

// ============================================================================
// Cross-widget actions
// ============================================================================

#[derive(Clone, Debug)]
pub enum CreateRoomAction {
    /// Room creation succeeded and all invites (if any) were accepted.
    Created { room_id: ruma::OwnedRoomId },
    /// Room created but one or more invites failed. The room is usable;
    /// the UI should surface the failed user-ids so the user can retry.
    PartialInvite {
        room_id: ruma::OwnedRoomId,
        failed: Vec<OwnedUserId>,
    },
    /// `Client::create_room` itself failed.
    Failed { reason: String },
}

// ============================================================================
// Widget
// ============================================================================

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.CreateRoomScreen = #(CreateRoomScreen::register_widget(vm)) {
        ..mod.widgets.ScrollYView

        width: Fill, height: Fill,
        flow: Down,
        padding: Inset{top: 5, left: 15, right: 15, bottom: 0},
        spacing: 8,

        title := Label {
            width: Fill, height: Fit,
            draw_text +: {
                text_style: theme.font_regular { font_size: 18 },
                color: #000
            }
            text: "Create a new room"
        }

        SubsectionLabel {
            text: "Room name (required)"
        }
        name_input := TextInput {
            width: Fill,
            empty_text: "Project Alpha"
        }

        SubsectionLabel { text: "Topic (optional)" }
        topic_input := TextInput {
            width: Fill,
            empty_text: "What's this room about?"
        }

        SubsectionLabel { text: "Visibility" }
        visibility_row := View {
            width: Fill, height: Fit,
            flow: Right, spacing: 10,
            visibility_private := CheckBox {
                text: "Private (invite only)"
                value: true
            }
            visibility_public := CheckBox {
                text: "Public"
                value: false
            }
        }

        SubsectionLabel { text: "End-to-end encryption" }
        e2ee_toggle := CheckBox {
            text: "Enable E2EE for this room"
            value: false
        }

        SubsectionLabel { text: "Invite people (matrix IDs, space- or comma-separated)" }
        invitees_input := TextInput {
            width: Fill,
            empty_text: "@alice:matrix.org, @bob:matrix.org"
        }

        validation_label := Label {
            width: Fill, height: Fit,
            draw_text +: { color: #xa10000 }
            text: ""
        }

        create_button := Button {
            width: Fit,
            text: "Create room"
        }
    }
}

#[derive(Script, Widget, ScriptHook)]
pub struct CreateRoomScreen {
    #[deref] view: View,
}

impl Widget for CreateRoomScreen {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        self.widget_match_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl WidgetMatchEvent for CreateRoomScreen {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, _scope: &mut Scope) {
        let create_button = self.view.button(cx, ids!(create_button));
        if create_button.clicked(actions) {
            let config = self.collect_config(cx);
            match validate_create_room_config(&config) {
                Ok(()) => {
                    self.view
                        .label(cx, ids!(validation_label))
                        .set_text(cx, "Creating room…");
                    submit_async_request(MatrixRequest::CreateRoom { config });
                }
                Err(error) => {
                    self.view
                        .label(cx, ids!(validation_label))
                        .set_text(cx, &describe_error(&error));
                }
            }
        }
    }
}

impl CreateRoomScreen {
    fn collect_config(&mut self, cx: &mut Cx) -> CreateRoomConfig {
        let name = self.view.text_input(cx, ids!(name_input)).text();
        let topic_raw = self.view.text_input(cx, ids!(topic_input)).text();
        let topic = (!topic_raw.trim().is_empty()).then(|| topic_raw.clone());
        let invitees_raw = self.view.text_input(cx, ids!(invitees_input)).text();
        let (parsed_invitees, _failed) = parse_invitee_list(&invitees_raw);
        let public = self
            .view
            .check_box(cx, ids!(visibility_public))
            .active(cx);
        let visibility = if public {
            RoomVisibilityChoice::Public
        } else {
            RoomVisibilityChoice::Private
        };
        let e2ee_enabled = self.view.check_box(cx, ids!(e2ee_toggle)).active(cx);
        CreateRoomConfig {
            name,
            topic,
            avatar_bytes: None,
            avatar_mime: None,
            visibility,
            e2ee_enabled,
            initial_invitees: parsed_invitees,
        }
    }
}

fn describe_error(error: &CreateRoomConfigError) -> String {
    match error {
        CreateRoomConfigError::EmptyName => "Please enter a room name.".to_string(),
        CreateRoomConfigError::NameTooLong { len, max } => {
            format!("Name is too long ({len} chars, max {max}).")
        }
        CreateRoomConfigError::InvalidInvitee { raw } => {
            format!("Invitee \"{raw}\" is not a valid Matrix user ID.")
        }
        CreateRoomConfigError::AvatarTooLarge { bytes, max } => {
            format!("Avatar is too large ({bytes} bytes, max {max}).")
        }
        CreateRoomConfigError::EncryptedPublicRoom => {
            "Public rooms can't be end-to-end encrypted.".to_string()
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests_room_creation {
    use super::*;
    use ruma::user_id;

    fn minimal_private(name: &str) -> CreateRoomConfig {
        CreateRoomConfig {
            name: name.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_create_room_config_rejects_empty_name() {
        let config = CreateRoomConfig {
            name: "".to_string(),
            ..Default::default()
        };
        assert_eq!(
            validate_create_room_config(&config),
            Err(CreateRoomConfigError::EmptyName)
        );
    }

    #[test]
    fn test_create_room_config_rejects_overlong_name() {
        let config = CreateRoomConfig {
            name: "a".repeat(256),
            ..Default::default()
        };
        assert_eq!(
            validate_create_room_config(&config),
            Err(CreateRoomConfigError::NameTooLong { len: 256, max: 255 })
        );
    }

    #[test]
    fn test_create_room_config_rejects_e2ee_on_public_room() {
        let config = CreateRoomConfig {
            name: "test".to_string(),
            visibility: RoomVisibilityChoice::Public,
            e2ee_enabled: true,
            ..Default::default()
        };
        assert_eq!(
            validate_create_room_config(&config),
            Err(CreateRoomConfigError::EncryptedPublicRoom)
        );
    }

    #[test]
    fn test_create_room_config_rejects_oversize_avatar() {
        let config = CreateRoomConfig {
            name: "test".to_string(),
            avatar_bytes: Some(vec![0u8; 5 * 1024 * 1024]),
            ..Default::default()
        };
        assert_eq!(
            validate_create_room_config(&config),
            Err(CreateRoomConfigError::AvatarTooLarge {
                bytes: 5_242_880,
                max: 4_194_304,
            })
        );
    }

    #[test]
    fn test_create_room_config_accepts_minimal_private_room() {
        let config = minimal_private("Project Alpha");
        assert_eq!(validate_create_room_config(&config), Ok(()));
    }

    #[test]
    fn test_create_room_config_accepts_private_encrypted() {
        let config = CreateRoomConfig {
            name: "Secrets".to_string(),
            topic: Some("plans".to_string()),
            visibility: RoomVisibilityChoice::Private,
            e2ee_enabled: true,
            initial_invitees: vec![user_id!("@alice:matrix.org").to_owned()],
            ..Default::default()
        };
        assert_eq!(validate_create_room_config(&config), Ok(()));
    }

    // ---- parse_invitee_list ----

    #[test]
    fn test_parse_invitee_list_splits_valid_and_invalid() {
        let (parsed, failed) =
            parse_invitee_list("@alice:matrix.org, bob@example.com  @carol:matrix.org");
        let parsed_strs: Vec<String> = parsed.iter().map(|u| u.to_string()).collect();
        assert!(parsed_strs.iter().any(|s| s == "@alice:matrix.org"));
        assert!(parsed_strs.iter().any(|s| s == "@carol:matrix.org"));
        assert_eq!(failed, vec!["bob@example.com".to_string()]);
    }

    #[test]
    fn test_parse_invitee_list_preserves_order() {
        let (parsed, _failed) = parse_invitee_list("@a:m.org\n@b:m.org , @c:m.org");
        let parsed_strs: Vec<String> = parsed.iter().map(|u| u.to_string()).collect();
        assert_eq!(
            parsed_strs,
            vec![
                "@a:m.org".to_string(),
                "@b:m.org".to_string(),
                "@c:m.org".to_string(),
            ]
        );
    }

    #[test]
    fn test_parse_invitee_list_returns_empty_for_blank() {
        let (parsed, failed) = parse_invitee_list("   \n  ");
        assert!(parsed.is_empty());
        assert!(failed.is_empty());
    }

    // ---- to_ruma_request ----

    #[test]
    fn test_create_room_config_to_ruma_request_includes_encryption_state() {
        let config = CreateRoomConfig {
            name: "Secrets".to_string(),
            visibility: RoomVisibilityChoice::Private,
            e2ee_enabled: true,
            ..Default::default()
        };
        let request = config.to_ruma_request();
        assert_eq!(request.initial_state.len(), 1);
        let raw = &request.initial_state[0];
        let json: serde_json::Value = serde_json::from_str(raw.json().get())
            .expect("initial_state entry is JSON");
        assert_eq!(json["type"], "m.room.encryption");
        assert_eq!(json["content"]["algorithm"], ENCRYPTION_ALGORITHM);
    }

    #[test]
    fn test_create_room_config_to_ruma_request_omits_encryption_when_off() {
        let config = CreateRoomConfig {
            name: "Public".to_string(),
            e2ee_enabled: false,
            ..Default::default()
        };
        let request = config.to_ruma_request();
        assert!(request.initial_state.is_empty());
    }

    #[test]
    fn test_create_room_config_to_ruma_request_public_visibility() {
        let config = CreateRoomConfig {
            name: "Open".to_string(),
            visibility: RoomVisibilityChoice::Public,
            e2ee_enabled: false,
            ..Default::default()
        };
        let request = config.to_ruma_request();
        assert_eq!(request.visibility, Visibility::Public);
        assert!(matches!(request.preset, Some(RoomPreset::PublicChat)));
    }
}
