//! A button displayed by the message text input box
//! that allows the user to send a message to Crew's chat server.

use makepad_widgets::*;

live_design! {
    link crew_enabled

    use link::theme::*;
    use link::widgets::*;

    use crate::shared::styles::*;

    pub CrewSendButton = <Button> {
        width: Fit,
        height: Fit,
        padding: {left: 8, right: 8, top: 6, bottom: 6}
        margin: {bottom: 7, left: 4, right: 0}

        draw_bg: {
            color: #4A90D9
            color_hover: #5BA0E9
            color_down: #3A80C9
            radius: 4.0
        }

        draw_text: {
            color: #FFFFFF
            text_style: <THEME_FONT_REGULAR> {
                font_size: 10.0
            }
        }

        text: "Crew"
    }
}
