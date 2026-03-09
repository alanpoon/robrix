//! The Crew settings screen widget.

use makepad_widgets::*;
use crate::shared::{popup_list::{enqueue_popup_notification, PopupKind}, styles::*};
use super::crew_state::{crew_settings, save_crew_settings, CrewSettings};

live_design! {
    use link::theme::*;
    use link::shaders::*;
    use link::widgets::*;

    use crate::shared::helpers::*;
    use crate::shared::styles::*;
    use crate::shared::icon_button::*;

    // The view containing all Crew-related settings.
    pub CrewSettingsScreen = {{CrewSettingsScreen}} {
        width: Fill, height: Fit
        flow: Down

        <TitleLabel> {
            text: "Crew Settings"
        }

        <SubsectionLabel> {
            text: "API Configuration:"
        }

        <View> {
            width: Fill, height: Fit
            flow: Down
            spacing: 10
            margin: {left: 5, top: 5}

            // Base URL input
            <View> {
                width: Fill, height: Fit
                flow: Down
                spacing: 5

                <Label> {
                    width: Fit, height: Fit
                    draw_text: {
                        text_style: <REGULAR_TEXT>{ font_size: 10 },
                        color: (COLOR_TEXT_IDLE)
                    }
                    text: "Base URL:"
                }

                base_url_input = <TextInput> {
                    width: 400, height: Fit
                    empty_message: "http://localhost:8080"
                    draw_bg: {
                        color: (COLOR_SECONDARY)
                    }
                    draw_text: {
                        text_style: <REGULAR_TEXT>{ font_size: 11 },
                        fn get_color(self) -> vec4 {
                            return (COLOR_TEXT_INPUT);
                        }
                    }
                }
            }

            // Authorization input
            <View> {
                width: Fill, height: Fit
                flow: Down
                spacing: 5

                <Label> {
                    width: Fit, height: Fit
                    draw_text: {
                        text_style: <REGULAR_TEXT>{ font_size: 10 },
                        color: (COLOR_TEXT_IDLE)
                    }
                    text: "Authorization:"
                }

                authorization_input = <TextInput> {
                    width: 400, height: Fit
                    empty_message: "Bearer my-secret"
                    draw_bg: {
                        color: (COLOR_SECONDARY)
                    }
                    draw_text: {
                        text_style: <REGULAR_TEXT>{ font_size: 11 },
                        fn get_color(self) -> vec4 {
                            return (COLOR_TEXT_INPUT);
                        }
                    }
                }
            }

            // Save button
            <View> {
                width: Fill, height: Fit
                flow: Right
                spacing: 10
                margin: {top: 10}

                save_button = <RobrixIconButton> {
                    width: Fit, height: Fit,
                    padding: 10,

                    draw_bg: {
                        color: (COLOR_ACTIVE_PRIMARY),
                        border_radius: 5
                    }
                    draw_icon: {
                        svg_file: (ICON_SAVE)
                        color: (COLOR_PRIMARY),
                    }
                    icon_walk: {width: 16, height: 16}
                    draw_text: {
                        color: (COLOR_PRIMARY),
                    }
                    text: "Save Settings"
                }

                reset_button = <RobrixIconButton> {
                    width: Fit, height: Fit,
                    padding: 10,

                    draw_bg: {
                        color: (COLOR_SECONDARY),
                        border_radius: 5
                    }
                    draw_text: {
                        color: (COLOR_TEXT_IDLE),
                    }
                    text: "Reset to Defaults"
                }
            }
        }

        <View> {
            width: Fill, height: Fit
            margin: {top: 10, left: 5, right: 5}

            <Label> {
                width: Fill, height: Fit
                flow: RightWrap,
                draw_text: {
                    wrap: Line,
                    color: (COLOR_TEXT_IDLE),
                    text_style: <MESSAGE_TEXT_STYLE>{ font_size: 9 },
                }
                text: "Note: Changes will take effect immediately. The base URL should not include the /api/chat path."
            }
        }
    }
}

/// The view containing all Crew-related settings.
#[derive(Live, LiveHook, Widget)]
pub struct CrewSettingsScreen {
    #[deref] view: View,
}

impl Widget for CrewSettingsScreen {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.match_event(cx, event);
        self.view.handle_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        // Load and display current settings
        if let Ok(settings) = crew_settings().lock() {
            self.text_input(ids!(base_url_input)).set_text(&settings.base_url);
            self.text_input(ids!(authorization_input)).set_text(&settings.authorization);
        }

        self.view.draw_walk(cx, scope, walk)
    }
}

impl MatchEvent for CrewSettingsScreen {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        // Save button clicked
        if self.button(ids!(save_button)).clicked(actions) {
            let base_url = self.text_input(ids!(base_url_input)).text();
            let authorization = self.text_input(ids!(authorization_input)).text();

            // Validate inputs
            let base_url = base_url.trim();
            let authorization = authorization.trim();

            if base_url.is_empty() {
                enqueue_popup_notification(
                    "Base URL cannot be empty.",
                    PopupKind::Warning,
                    Some(3.0),
                );
                return;
            }

            if authorization.is_empty() {
                enqueue_popup_notification(
                    "Authorization cannot be empty.",
                    PopupKind::Warning,
                    Some(3.0),
                );
                return;
            }

            // Update settings
            let new_settings = CrewSettings {
                base_url: base_url.to_string(),
                authorization: authorization.to_string(),
            };

            // Save to global state
            if let Ok(mut settings) = crew_settings().lock() {
                *settings = new_settings.clone();
            }

            // Save to persistent storage
            if let Err(e) = save_crew_settings(new_settings) {
                enqueue_popup_notification(
                    format!("Failed to save Crew settings: {}", e),
                    PopupKind::Error,
                    None,
                );
            } else {
                enqueue_popup_notification(
                    "Crew settings saved successfully.",
                    PopupKind::Success,
                    Some(3.0),
                );
            }
        }

        // Reset button clicked
        if self.button(ids!(reset_button)).clicked(actions) {
            let default_settings = CrewSettings::default();

            self.text_input(ids!(base_url_input)).set_text(&default_settings.base_url);
            self.text_input(ids!(authorization_input)).set_text(&default_settings.authorization);

            // Update global state
            if let Ok(mut settings) = crew_settings().lock() {
                *settings = default_settings.clone();
            }

            // Save to persistent storage
            if let Err(e) = save_crew_settings(default_settings) {
                enqueue_popup_notification(
                    format!("Failed to reset Crew settings: {}", e),
                    PopupKind::Error,
                    None,
                );
            } else {
                enqueue_popup_notification(
                    "Crew settings reset to defaults.",
                    PopupKind::Success,
                    Some(3.0),
                );
            }

            self.redraw(cx);
        }
    }
}

pub trait CrewSettingsScreenWidgetExt {
    fn crew_settings_screen(&self, path: &[LiveId]) -> CrewSettingsScreenRef;
}

impl CrewSettingsScreenWidgetExt for WidgetRef {
    fn crew_settings_screen(&self, path: &[LiveId]) -> CrewSettingsScreenRef {
        self.widget(path).crew_settings_screen_ref()
    }
}

impl CrewSettingsScreen {
    /// Populate the settings screen with current values.
    pub fn show(&mut self, cx: &mut Cx) {
        if let Ok(settings) = crew_settings().lock() {
            self.text_input(ids!(base_url_input)).set_text(&settings.base_url);
            self.text_input(ids!(authorization_input)).set_text(&settings.authorization);
        }
        self.redraw(cx);
    }
}
