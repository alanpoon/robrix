//! This module defines a generic collapsible header widget for small state events
//! that can be used in any context without requiring specific scope props.
//!
//! This widget can be clicked to toggle between expanded and collapsed states.

use makepad_widgets::*;
use super::unread_badge::UnreadBadgeWidgetExt;
use super::collapsible_header::HeaderCategory;

live_design! {
    use link::theme::*;
    use link::widgets::*;
    use link::shaders::*;

    use crate::shared::styles::*;
    use crate::shared::unread_badge::*;

    ICON_COLLAPSE = dep("crate://self/resources/icons/triangle_fill.svg")

    COLOR_HEADER_FG = #F;
    COLOR_HEADER_BG = (COLOR_ROBRIX_PURPLE); // the purple color from the Robrix logo

    pub CollapsibleHeaderSmallState = {{CollapsibleHeaderSmallState}}<RoundedView> {
        width: Fill,
        height: 35,
        align: { x: 0.0, y: 0.5 },
        margin: {top: 3, bottom: 3, left: 0, right: 0},
        padding: 5
        flow: Right,

        cursor: Hand,
        draw_bg: {
            border_radius: 4.0,
            color: (COLOR_HEADER_BG)
        }

        collapse_icon = <IconRotated> {
            margin: {left: 5, right: 8, top: 0, bottom: 0},
            draw_icon: {
                svg_file: (ICON_COLLAPSE),
                rotation_angle: 180.0, // start in the "expanded" state
                color: (COLOR_HEADER_FG),
            }
            icon_walk: { width: 14, height: Fit, margin: 0, }
        }
        label = <Label> {
            padding: 0,
            width: Fill,
            height: Fit,
            text: "",
            draw_text: {
                text_style: <REGULAR_TEXT>{font_size: 11},
                color: (COLOR_HEADER_FG),
            }
        }
        unread_badge = <UnreadBadge> {
            margin: {right: 5.5},
        }
    }
}

#[derive(Clone, Debug, DefaultNone)]
pub enum CollapsibleHeaderSmallStateAction {
    /// The header was clicked to toggled its expanded/collapsed state.
    Toggled {
        category: HeaderCategory,
        group_id: usize, // Timeline index where this group starts
    },
    None,
}

#[derive(Live, LiveHook, Widget)]
pub struct CollapsibleHeaderSmallState {
    #[deref] view: View,
    #[rust(true)] is_expanded: bool,
    #[rust] category: HeaderCategory,
    #[rust] group_id: usize,
}

impl Widget for CollapsibleHeaderSmallState {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        // Handle hits on this view as a whole before passing the event to the inner view.
        match event.hits(cx, self.view.area()) {
            Hit::FingerDown(..) => {
                cx.set_key_focus(self.view.area());
            }
            Hit::FingerUp(fe) => {
                if fe.is_over && fe.is_primary_hit() && fe.was_tap() {
                    self.toggle_collapse(cx, scope);
                }
            }
            _ => { }
        }
        self.view.handle_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        let angle = if self.is_expanded {
            180.0
        } else {
            90.0
        };
        self.icon(ids!(collapse_icon)).apply_over(
            cx,
            live! {
                draw_icon: { rotation_angle: (angle) }
            },
        );
        self.view.draw_walk(cx, scope, walk)
    }
}

impl CollapsibleHeaderSmallState {
    fn toggle_collapse(&mut self, cx: &mut Cx, scope: &mut Scope) {
        self.is_expanded = !self.is_expanded;
        self.redraw(cx);
        cx.widget_action(
            self.widget_uid(),
            &scope.path,
            CollapsibleHeaderSmallStateAction::Toggled {
                category: self.category,
                group_id: self.group_id,
            },
        );
    }
}

impl CollapsibleHeaderSmallStateRef {
    /// Sets the category, expanded state, and group ID of the header.
    pub fn set_details(
        &self,
        cx: &mut Cx,
        is_expanded: bool,
        category: HeaderCategory,
        group_id: usize,
        num_unread_mentions: u64,
    ) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.is_expanded = is_expanded;
            inner.category = category;
            inner.group_id = group_id;
            inner.label(ids!(label)).set_text(cx, category.as_str());
            inner.unread_badge(ids!(unread_badge)).update_counts(num_unread_mentions, 0);
        }
    }
}