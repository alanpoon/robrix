//! Modal "Account Management" screen — opened by the **Manage Account**
//! button in the Settings → Account page. Mirrors the matrix.org account
//! portal layout: a user-info card on top, then a tab bar (`Account` |
//! `Devices`) controlling a `PageFlip` of two sub-screens.
//!
//! Account tab:
//!   - User info card (avatar letter, display name, matrix id)
//!   - Email row (read-only — adding email needs UIA + 3pid verification
//!     which is out of scope for this iteration)
//!   - "Change password" button → opens the homeserver's account portal in
//!     the system browser (OIDC accounts get the OIDC account-management URL
//!     when available; password accounts get the homeserver root, which is
//!     where Synapse exposes its account-management UI).
//!   - "Log out" button → fires the existing `LogoutConfirmModalAction::Open`,
//!     so the existing logout-confirm modal handles the rest.
//!
//! Devices tab:
//!   - Embeds the already-built `DevicesScreen` widget.

use makepad_widgets::*;

use crate::logout::logout_confirm_modal::LogoutConfirmModalAction;
use crate::shared::popup_list::{PopupKind, enqueue_popup_notification};
use crate::sliding_sync::{
    AccountDataAction, ChangePasswordOutcome, MatrixRequest, get_client, submit_async_request,
};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.AccountManagementModal = #(AccountManagementModal::register_widget(vm)) {
        width: Fit, height: Fit

        // White wrapper drawn behind everything — outer widget can't host the
        // background reliably (it's the registered widget root, properties on
        // it don't always propagate to its draw step inside a Modal). Same
        // wrapper-with-bg pattern the ConfirmationModal uses.
        am_wrapper := RoundedView {
            width: 720, height: Fit
            flow: Down
            padding: Inset{top: 24, bottom: 24, left: 32, right: 32}
            show_bg: true
            draw_bg +: {
                color: (COLOR_PRIMARY)
                border_radius: 8.0
            }

        // ─────────────────────────── header ────────────────────────────
        am_header_row := View {
            width: Fill, height: Fit
            flow: Right
            align: Align{y: 0.5}
            spacing: 8

            am_header_title := Label {
                width: Fill, height: Fit
                text: "Your account"
                draw_text +: {
                    color: #x101012
                    text_style: theme.font_bold { font_size: 20.0 }
                }
            }
            am_close_button := RobrixIconButton {
                width: Fit, height: Fit,
                padding: 12,
                spacing: 0
                align: Align{x: 0.5, y: 0.5}
                icon_walk: Walk{width: 18, height: 18, margin: 0}
                draw_icon.svg: (ICON_CLOSE)
                draw_icon.color: #666
                draw_bg +: {
                    border_size: 0
                    color: #0000
                    color_hover: #00000015
                    color_down: #00000025
                }
            }
        }

        // ───────────────────────── user info card ─────────────────────
        am_user_card := RoundedView {
            width: Fill, height: Fit
            flow: Right
            align: Align{y: 0.5}
            padding: Inset{top: 12, bottom: 12, left: 14, right: 14}
            margin: Inset{top: 12, bottom: 12}
            spacing: 12
            show_bg: true
            draw_bg +: {
                color: #xFFFFFF
                border_size: 1.0
                border_color: #xE5E5EA
                border_radius: 10.0
            }

            // Big rounded "letter" avatar — first character of the display name.
            am_user_avatar := RoundedView {
                width: 56, height: 56
                align: Align{x: 0.5, y: 0.5}
                show_bg: true
                draw_bg +: {
                    color: #xE9D8FD
                    border_radius: 28.0
                }
                am_user_avatar_letter := Label {
                    text: "?"
                    draw_text +: {
                        color: #x5B21B6
                        text_style: theme.font_bold { font_size: 26.0 }
                    }
                }
            }

            am_user_text_col := View {
                width: Fill, height: Fit
                flow: Down
                spacing: 4

                am_user_display_name := Label {
                    width: Fill, height: Fit
                    text: "—"
                    draw_text +: {
                        color: #x101012
                        text_style: theme.font_bold { font_size: 16.0 }
                    }
                }
                am_user_matrix_id := Label {
                    width: Fill, height: Fit
                    text: ""
                    draw_text +: {
                        color: #x606066
                        text_style: theme.font_regular { font_size: 13.0 }
                    }
                }
            }
        }

        // ──────────────────────────── tab bar ─────────────────────────
        am_tab_bar := View {
            width: Fill, height: Fit
            flow: Right
            spacing: 12
            align: Align{y: 0.5}
            margin: Inset{top: 4, bottom: 12}

            am_tab_account_button := RobrixNeutralIconButton {
                text: "Settings"
                width: Fit, height: 32
            }
            am_tab_devices_button := RobrixNeutralIconButton {
                text: "Devices"
                width: Fit, height: 32
            }
        }

        // ──────────────────────── content (page flip) ─────────────────
        am_sections := PageFlip {
            width: Fill, height: Fit
            lazy_init: true
            active_page: @am_account_page

            am_account_page := View {
                width: Fill, height: Fit
                flow: Down
                spacing: 12

                // Account password section
                am_pw_section_title := Label {
                    text: "Account password"
                    draw_text +: {
                        color: #x101012
                        text_style: theme.font_bold { font_size: 14.0 }
                    }
                }
                am_pw_section := RoundedView {
                    width: Fill, height: Fit
                    flow: Down
                    padding: Inset{top: 12, bottom: 12, left: 14, right: 14}
                    show_bg: true
                    draw_bg +: {
                        color: #xF8F8FA
                        border_radius: 8.0
                    }
                    am_pw_body := Label {
                        width: Fill, height: Fit
                        text: "Change your account password. Your homeserver must support password authentication (Synapse with password auth enabled)."
                        draw_text +: {
                            color: #x404044
                            text_style: theme.font_regular { font_size: 12.0 }
                        }
                    }
                    am_change_password_button := RobrixNeutralIconButton {
                        text: "Change password"
                        width: Fit, height: 32
                        margin: Inset{top: 10}
                    }

                    // Expanded form — hidden until the user clicks the
                    // "Change password" button above.
                    am_pw_form := View {
                        visible: false
                        width: Fill, height: Fit
                        flow: Down
                        spacing: 6
                        margin: Inset{top: 12}

                        am_current_pw_label := Label {
                            text: "Current Password"
                            draw_text +: {
                                color: #x101012
                                text_style: theme.font_bold { font_size: 12.0 }
                            }
                        }
                        am_current_pw_input := RobrixTextInput {
                            width: Fill, height: Fit
                            flow: Right
                            empty_text: ""
                            is_password: true
                            padding: Inset{top: 5, bottom: 5, left: 10, right: 10}
                        }

                        am_new_pw_label := Label {
                            text: "New Password"
                            margin: Inset{top: 8}
                            draw_text +: {
                                color: #x101012
                                text_style: theme.font_bold { font_size: 12.0 }
                            }
                        }
                        am_new_pw_input := RobrixTextInput {
                            width: Fill, height: Fit
                            flow: Right
                            empty_text: ""
                            is_password: true
                            padding: Inset{top: 5, bottom: 5, left: 10, right: 10}
                        }

                        am_pw_strength_label := Label {
                            text: "Password strength"
                            margin: Inset{top: 8}
                            draw_text +: {
                                color: #x606066
                                text_style: theme.font_regular { font_size: 11.0 }
                            }
                        }
                        am_pw_strength_row := View {
                            width: Fill, height: Fit
                            flow: Right
                            spacing: 6

                            am_pw_strength_box1 := View {
                                width: Fill, height: 8
                                show_bg: true
                                draw_bg +: { color: #xE5E5EA, border_radius: 3.0 }
                            }
                            am_pw_strength_box2 := View {
                                width: Fill, height: 8
                                show_bg: true
                                draw_bg +: { color: #xE5E5EA, border_radius: 3.0 }
                            }
                            am_pw_strength_box3 := View {
                                width: Fill, height: 8
                                show_bg: true
                                draw_bg +: { color: #xE5E5EA, border_radius: 3.0 }
                            }
                            am_pw_strength_box4 := View {
                                width: Fill, height: 8
                                show_bg: true
                                draw_bg +: { color: #xE5E5EA, border_radius: 3.0 }
                            }
                        }
                        am_pw_strength_text := Label {
                            text: "—"
                            margin: Inset{top: 4}
                            draw_text +: {
                                color: #x606066
                                text_style: theme.font_regular { font_size: 11.0 }
                            }
                        }

                        am_pw_form_buttons := View {
                            width: Fill, height: Fit
                            flow: Right
                            spacing: 8
                            margin: Inset{top: 12}

                            am_pw_confirm_button := RobrixPositiveIconButton {
                                text: "Change password"
                                width: Fit, height: 32
                            }
                            am_pw_cancel_button := RobrixNeutralIconButton {
                                text: "Cancel"
                                width: Fit, height: 32
                            }
                        }
                    }
                }

                // Logout section
                am_logout_section_title := Label {
                    text: "Session"
                    margin: Inset{top: 8}
                    draw_text +: {
                        color: #x101012
                        text_style: theme.font_bold { font_size: 14.0 }
                    }
                }
                am_logout_section := RoundedView {
                    width: Fill, height: Fit
                    flow: Down
                    padding: Inset{top: 12, bottom: 12, left: 14, right: 14}
                    show_bg: true
                    draw_bg +: {
                        color: #xF8F8FA
                        border_radius: 8.0
                    }
                    am_logout_body := Label {
                        width: Fill, height: Fit
                        text: "Sign out of this device. Your encrypted messages stay safe — make sure you have your recovery key first."
                        draw_text +: {
                            color: #x404044
                            text_style: theme.font_regular { font_size: 12.0 }
                        }
                    }
                    am_logout_button := RobrixNegativeIconButton {
                        text: "Log out"
                        width: Fit, height: 32
                        margin: Inset{top: 10}
                    }
                }
            }

            am_devices_page := View {
                width: Fill, height: 520
                flow: Down

                am_devices_screen := DevicesScreen {}
            }
        }
        } // end am_wrapper
    }
}

// ────────────────────────────── actions ──────────────────────────────

/// Top-level action to open / close the modal. Handled by `app.rs`.
#[derive(Clone, Debug)]
pub enum AccountManagementAction {
    Open,
    Close,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum AmTab {
    #[default]
    Account,
    Devices,
}

// ─────────────────────────── modal widget ────────────────────────────

#[derive(Script, ScriptHook, Widget)]
pub struct AccountManagementModal {
    #[deref] view: View,
    #[rust] selected: AmTab,
    /// One-shot init flag so we only pull the user info from the SDK once
    /// per Open.
    #[rust] populated: bool,
    /// True while a ChangePassword request is in flight — disables the
    /// confirm button to prevent double-submit.
    #[rust] in_flight: bool,
}

impl Widget for AccountManagementModal {
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        if !self.populated {
            self.populate_user_info(cx);
            self.populated = true;
        }
        self.view.draw_walk(cx, scope, walk)
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);

        if let Event::Actions(actions) = event {
            // Close button
            if self.view.button(cx, ids!(am_close_button)).clicked(actions) {
                cx.action(AccountManagementAction::Close);
            }

            // Tab buttons
            if self.view.button(cx, ids!(am_tab_account_button)).clicked(actions) {
                self.set_tab(cx, AmTab::Account);
            }
            if self.view.button(cx, ids!(am_tab_devices_button)).clicked(actions) {
                self.set_tab(cx, AmTab::Devices);
            }

            // Click "Change password" → expand the in-modal form.
            if self
                .view
                .button(cx, ids!(am_change_password_button))
                .clicked(actions)
            {
                self.show_pw_form(cx);
            }

            // Cancel inside the form → hide & clear.
            if self
                .view
                .button(cx, ids!(am_pw_cancel_button))
                .clicked(actions)
            {
                self.hide_pw_form(cx);
            }

            // Confirm inside the form → validate and submit.
            if self
                .view
                .button(cx, ids!(am_pw_confirm_button))
                .clicked(actions)
            {
                self.confirm_change_password(cx);
            }

            // Live strength meter — react to keystrokes on the new password.
            if let Some(text) =
                self.view.text_input(cx, ids!(am_new_pw_input)).changed(actions)
            {
                self.update_strength(cx, &text);
            }

            // Log out → existing modal flow
            if self.view.button(cx, ids!(am_logout_button)).clicked(actions) {
                cx.action(LogoutConfirmModalAction::Open);
            }

            // Listen for the change-password result from the Matrix worker.
            for action in actions {
                if let Some(AccountDataAction::ChangePasswordResult(outcome)) =
                    action.downcast_ref()
                {
                    self.apply_change_password_result(cx, outcome);
                }
            }
        }
    }
}

impl AccountManagementModalRef {
    /// Refresh the user info + reset to the Account tab. Called by `app.rs`
    /// whenever the modal opens.
    pub fn reset(&self) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.populated = false;
            inner.selected = AmTab::Account;
            inner.in_flight = false;
        }
    }
}

impl AccountManagementModal {

    fn set_tab(&mut self, cx: &mut Cx, tab: AmTab) {
        if self.selected == tab {
            return;
        }
        self.selected = tab;
        self.view.page_flip(cx, ids!(am_sections)).set_active_page(
            cx,
            match tab {
                AmTab::Account => id!(am_account_page),
                AmTab::Devices => id!(am_devices_page),
            },
        );
        self.view.redraw(cx);
    }

    fn populate_user_info(&mut self, cx: &mut Cx) {
        let Some(client) = get_client() else { return };
        let Some(session) = client.session_meta() else { return };
        let user_id = session.user_id.to_string();
        // Localpart of "@alice:matrix.org" → "alice".
        let localpart = user_id
            .strip_prefix('@')
            .and_then(|s| s.split(':').next())
            .unwrap_or(&user_id);
        let avatar_letter = localpart
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string());

        self.view
            .label(cx, ids!(am_user_avatar_letter))
            .set_text(cx, &avatar_letter);
        self.view
            .label(cx, ids!(am_user_display_name))
            .set_text(cx, localpart);
        self.view
            .label(cx, ids!(am_user_matrix_id))
            .set_text(cx, &user_id);
    }

    /// Expand the inline password-change form.
    fn show_pw_form(&mut self, cx: &mut Cx) {
        self.view.view(cx, ids!(am_pw_form)).set_visible(cx, true);
        // Reset the strength visual to empty.
        self.apply_strength_score(cx, 0);
        self.view
            .label(cx, ids!(am_pw_strength_text))
            .set_text(cx, "—");
        self.view.redraw(cx);
    }

    /// Hide the form and clear both inputs.
    fn hide_pw_form(&mut self, cx: &mut Cx) {
        self.view
            .text_input(cx, ids!(am_current_pw_input))
            .set_text(cx, "");
        self.view
            .text_input(cx, ids!(am_new_pw_input))
            .set_text(cx, "");
        self.view.view(cx, ids!(am_pw_form)).set_visible(cx, false);
        self.apply_strength_score(cx, 0);
        self.view.redraw(cx);
    }

    /// Recompute the strength score and update the 4 boxes + status label.
    fn update_strength(&mut self, cx: &mut Cx, new_password: &str) {
        let score = compute_strength_score(new_password);
        self.apply_strength_score(cx, score);
        let label = match score {
            0 => "Too weak",
            1 => "Weak",
            2 => "Fair",
            3 => "Good",
            _ => "Strong",
        };
        self.view
            .label(cx, ids!(am_pw_strength_text))
            .set_text(cx, label);
    }

    /// Paint exactly `score` boxes filled and the remaining ones gray.
    fn apply_strength_score(&mut self, cx: &mut Cx, score: u8) {
        let filled = strength_color(score);
        let empty = 0xE5E5EAu32;
        let colors: [u32; 4] = [
            if score >= 1 { filled } else { empty },
            if score >= 2 { filled } else { empty },
            if score >= 3 { filled } else { empty },
            if score >= 4 { filled } else { empty },
        ];
        let ids_list = [
            ids!(am_pw_strength_box1),
            ids!(am_pw_strength_box2),
            ids!(am_pw_strength_box3),
            ids!(am_pw_strength_box4),
        ];
        for (path, hex) in ids_list.iter().zip(colors.iter()) {
            let mut box_ref = self.view.view(cx, *path);
            let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
            let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
            let b = (hex & 0xFF) as f32 / 255.0;
            script_apply_eval!(cx, box_ref, {
                draw_bg +: { color: vec4(#(r), #(g), #(b), 1.0) }
            });
        }
        self.view.redraw(cx);
    }

    /// Read the form, validate, and dispatch a Matrix `change_password` call.
    /// (Network call is a TODO — currently we just show what would be sent.)
    fn confirm_change_password(&mut self, cx: &mut Cx) {
        let current = self
            .view
            .text_input(cx, ids!(am_current_pw_input))
            .text();
        let new_pw = self.view.text_input(cx, ids!(am_new_pw_input)).text();

        if current.is_empty() {
            enqueue_popup_notification(
                "Please enter your current password.".to_string(),
                PopupKind::Warning,
                Some(5.0),
            );
            return;
        }
        if new_pw.is_empty() {
            enqueue_popup_notification(
                "Please enter a new password.".to_string(),
                PopupKind::Warning,
                Some(5.0),
            );
            return;
        }
        if compute_strength_score(&new_pw) < 2 {
            enqueue_popup_notification(
                "New password is too weak — needs at least 8 characters and one digit.".to_string(),
                PopupKind::Warning,
                Some(6.0),
            );
            return;
        }
        if current == new_pw {
            enqueue_popup_notification(
                "New password must be different from the current one.".to_string(),
                PopupKind::Warning,
                Some(5.0),
            );
            return;
        }
        if get_client().is_none() {
            enqueue_popup_notification(
                "Not signed in.".to_string(),
                PopupKind::Warning,
                Some(4.0),
            );
            return;
        }
        // Dispatch the actual change to the Matrix worker. The result comes
        // back asynchronously as `AccountDataAction::ChangePasswordResult`
        // and is handled in `handle_event`.
        submit_async_request(MatrixRequest::ChangePassword {
            current_password: current,
            new_password: new_pw,
        });
        self.in_flight = true;
        self.view
            .button(cx, ids!(am_pw_confirm_button))
            .set_enabled(cx, false);
        self.view
            .label(cx, ids!(am_pw_strength_text))
            .set_text(cx, "Submitting…");
        self.view.redraw(cx);
    }

    fn apply_change_password_result(&mut self, cx: &mut Cx, outcome: &ChangePasswordOutcome) {
        self.in_flight = false;
        self.view
            .button(cx, ids!(am_pw_confirm_button))
            .set_enabled(cx, true);
        match outcome {
            ChangePasswordOutcome::Success => {
                enqueue_popup_notification(
                    "Password changed successfully.".to_string(),
                    PopupKind::Success,
                    Some(5.0),
                );
                self.hide_pw_form(cx);
            }
            ChangePasswordOutcome::WrongCurrentPassword => {
                enqueue_popup_notification(
                    "Current password is wrong.".to_string(),
                    PopupKind::Error,
                    Some(6.0),
                );
                // Keep the form open so the user can retry.
                self.view
                    .text_input(cx, ids!(am_current_pw_input))
                    .set_text(cx, "");
                self.view
                    .label(cx, ids!(am_pw_strength_text))
                    .set_text(cx, "—");
                self.view.redraw(cx);
            }
            ChangePasswordOutcome::WeakPassword(msg) => {
                enqueue_popup_notification(
                    format!("Server rejected new password: {msg}"),
                    PopupKind::Warning,
                    Some(8.0),
                );
                self.view.redraw(cx);
            }
            ChangePasswordOutcome::NotSupported => {
                enqueue_popup_notification(
                    "This account doesn't support in-app password change. \
                     Use your homeserver's account portal instead."
                        .to_string(),
                    PopupKind::Warning,
                    Some(8.0),
                );
                self.hide_pw_form(cx);
            }
            ChangePasswordOutcome::Error(msg) => {
                enqueue_popup_notification(
                    format!("Failed to change password: {msg}"),
                    PopupKind::Error,
                    Some(8.0),
                );
                self.view.redraw(cx);
            }
        }
    }
}

// ────────────────────────── strength heuristic ──────────────────────────

/// Returns a score in `0..=4` based on length + character variety.
fn compute_strength_score(pw: &str) -> u8 {
    if pw.is_empty() {
        return 0;
    }
    let mut score = 0u8;
    if pw.len() >= 8 {
        score += 1;
    }
    if pw.chars().any(|c| c.is_ascii_digit()) {
        score += 1;
    }
    let has_upper = pw.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = pw.chars().any(|c| c.is_ascii_lowercase());
    if has_upper && has_lower {
        score += 1;
    }
    let has_special = pw.chars().any(|c| !c.is_ascii_alphanumeric());
    if has_special || pw.len() >= 12 {
        score += 1;
    }
    score
}

/// Color used for filled strength boxes at the given score.
fn strength_color(score: u8) -> u32 {
    match score {
        1 => 0xCC3333, // red
        2 => 0xD4A017, // amber
        3 => 0x88B43F, // lime
        _ => 0x33A852, // green (score 4)
    }
}
