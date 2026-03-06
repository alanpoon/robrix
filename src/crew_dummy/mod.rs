//! This module provides dummy Crew-related widgets that do nothing.
//!
//! We only need to define dummy widgets for Crew-specific widgets that are used
//! from non-Crew DSL code, i.e., any widgets that exist on the boundary between
//! Crew and non-Crew code.
//!
//! The real Crew widgets are all defined in the `crew_enabled` namespace,
//! and their live_design DSL blocks all start with `link crew_enabled`,
//! which declares the namespace that they exist within.
//!
//! The "active" namespace is selected via the `cx.link()` call in `App::live_register()`,
//! which connects the `crew_link` DSL namespace to the `crew_disabled` namespace
//! defined in this module, only when the `crew` feature is not enabled.
//!
//! This allows the rest of the application's DSL to directly use Crew widgets,
//! but the widgets that actually get imported under the `crew_link` namespace
//! will be replaced with these dummy widgets when the `crew` feature is not enabled.

use makepad_widgets::*;

live_design! {
    link crew_disabled

    use link::theme::*;
    use link::shaders::*;
    use link::widgets::*;

    use crate::shared::styles::*;

    pub CrewSendButton = <View> {
        visible: false
    }
}
