//! A dropdown menu that appears when the user clicks on the RoomsListHeader.
//! Provides filter and sort options for the rooms list.

use makepad_widgets::*;

use crate::home::rooms_list_header::{RoomFilterOption, RoomSortOption, RoomsListHeaderAction};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*


    mod.widgets.ROOMS_HEADER_DROPDOWN_BUTTON_HEIGHT = 35
    mod.widgets.ROOMS_HEADER_DROPDOWN_WIDTH = 200

    mod.widgets.RoomsHeaderDropdownButton = Button {
        height: (mod.widgets.ROOMS_HEADER_DROPDOWN_BUTTON_HEIGHT)
        width: Fill,
        margin: 0,
        padding: Inset{left: 10, right: 10, top: 8, bottom: 8}
        spacing: 8,
        align: Align{x: 0, y: 0.5}
        icon_walk: Walk{width: 16, height: 16, margin: Inset{right: 3}}
        draw_bg +: {
            border_radius: 4.0
            border_size: 0.0
            color: (COLOR_PRIMARY)
            color_hover: #EBEBEB
            color_down: #DCDCDC
            color_focus: (COLOR_PRIMARY)
            color_disabled: (COLOR_PRIMARY)
        }
        draw_icon +: {
            color: #333
            color_hover: #333
            color_down: #333
        }
        draw_text +: {
            color: uniform(#000)
            color_hover: uniform(#000)
            color_down: uniform(#000)
            color_focus: uniform(#000)
            color_disabled: uniform(#888)
            text_style: REGULAR_TEXT {}
        }
    }

    // Remove the unused selected button style variant

    mod.widgets.RoomsListHeaderDropdown = set_type_default() do #(RoomsListHeaderDropdown::register_widget(vm)) {
        ..mod.widgets.SolidView

        visible: false,
        flow: Overlay,
        width: Fill,
        height: Fill,
        cursor: MouseCursor.Default,
        align: Align{x: 0, y: 0}

        show_bg: true
        draw_bg +: {
            color: #0000004d
        }

        main_content := RoundedView {
            flow: Down
            width: (mod.widgets.ROOMS_HEADER_DROPDOWN_WIDTH),
            height: Fit,
            padding: 5
            spacing: 0,
            align: Align{x: 0, y: 0}

            show_bg: true
            draw_bg +: {
                color: (COLOR_PRIMARY)
                border_radius: 5.0
                border_size: 0.5
                border_color: #888
            }

            // Section: Filter
            section_filter := Label {
                width: Fill,
                height: Fit,
                padding: Inset{left: 10, top: 5, bottom: 3}
                text: "Filter"
                draw_text +: {
                    color: #888
                    text_style: TEXT_SUB {}
                }
            }

            filter_all_button := mod.widgets.RoomsHeaderDropdownButton {
                draw_icon.svg: (ICON_HOME)
                text: "All Rooms"
            }

            filter_unread_button := mod.widgets.RoomsHeaderDropdownButton {
                draw_icon.svg: (ICON_INFO)
                text: "Unread"
            }

            filter_favorites_button := mod.widgets.RoomsHeaderDropdownButton {
                draw_icon.svg: (ICON_PIN)
                text: "Favorites"
            }

            filter_people_button := mod.widgets.RoomsHeaderDropdownButton {
                draw_icon.svg: (ICON_ADD_USER)
                text: "People"
            }

            divider1 := LineH {
                margin: Inset{top: 5, bottom: 5}
                width: Fill,
            }

            // Section: Sort
            section_sort := Label {
                width: Fill,
                height: Fit,
                padding: Inset{left: 10, top: 3, bottom: 3}
                text: "Sort by"
                draw_text +: {
                    color: #888
                    text_style: TEXT_SUB {}
                }
            }

            sort_activity_button := mod.widgets.RoomsHeaderDropdownButton {
                draw_icon.svg: (ICON_HIERARCHY)
                text: "Activity"
            }

            sort_alphabetical_button := mod.widgets.RoomsHeaderDropdownButton {
                draw_icon.svg: (ICON_SQUARES)
                text: "A-Z"
            }

            sort_unread_button := mod.widgets.RoomsHeaderDropdownButton {
                draw_icon.svg: (ICON_INFO)
                text: "Unread First"
            }

            divider2 := LineH {
                margin: Inset{top: 5, bottom: 5}
                width: Fill,
            }

            // Section: Actions
            section_actions := Label {
                width: Fill,
                height: Fit,
                padding: Inset{left: 10, top: 3, bottom: 3}
                text: "Actions"
                draw_text +: {
                    color: #888
                    text_style: TEXT_SUB {}
                }
            }

            mark_all_read_button := mod.widgets.RoomsHeaderDropdownButton {
                draw_icon.svg: (ICON_CHECKMARK)
                text: "Mark all as read"
            }
        }
    }
}

/// State for the dropdown menu.
#[derive(Clone, Debug, Default)]
pub struct RoomsListHeaderDropdownState {
    pub filter: RoomFilterOption,
    pub sort: RoomSortOption,
}

/// The number of focusable menu items (4 filter + 3 sort + 1 action).
const MENU_ITEM_COUNT: usize = 8;

#[derive(Script, ScriptHook, Widget)]
pub struct RoomsListHeaderDropdown {
    #[deref] view: View,
    #[source] source: ScriptObjectRef,
    #[rust] state: Option<RoomsListHeaderDropdownState>,
    /// Index of the currently focused menu item for keyboard navigation.
    #[rust] focused_index: usize,
}

impl Widget for RoomsListHeaderDropdown {
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        if self.state.is_none() {
            self.visible = false;
        };
        self.view.draw_walk(cx, scope, walk)
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if !self.visible { return; }
        self.view.handle_event(cx, event, scope);

        // Close logic
        let area = self.view.area();
        let close_menu = {
            event.back_pressed()
            || match event.hits_with_capture_overload(cx, area, true) {
                Hit::KeyUp(key) => key.key_code == KeyCode::Escape,
                Hit::FingerUp(fue) if fue.is_over => {
                    !self.view(cx, ids!(main_content)).area().rect(cx).contains(fue.abs)
                }
                Hit::FingerScroll(_) => true,
                _ => false,
            }
        };

        if close_menu {
            self.close(cx);
            return;
        }

        // Handle keyboard navigation
        if let Event::KeyDown(key_event) = event {
            match key_event.key_code {
                KeyCode::ArrowDown => {
                    self.focused_index = (self.focused_index + 1) % MENU_ITEM_COUNT;
                    self.redraw(cx);
                    return;
                }
                KeyCode::ArrowUp => {
                    self.focused_index = if self.focused_index == 0 {
                        MENU_ITEM_COUNT - 1
                    } else {
                        self.focused_index - 1
                    };
                    self.redraw(cx);
                    return;
                }
                KeyCode::ReturnKey => {
                    self.activate_focused_item(cx);
                    return;
                }
                _ => {}
            }
        }

        self.widget_match_event(cx, event, scope);
    }
}

impl WidgetMatchEvent for RoomsListHeaderDropdown {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, _scope: &mut Scope) {
        let Some(_state) = self.state.as_ref() else { return };
        let mut close_menu = false;
        let mut new_filter: Option<RoomFilterOption> = None;
        let mut new_sort: Option<RoomSortOption> = None;

        // Filter buttons
        if self.button(cx, ids!(filter_all_button)).clicked(actions) {
            new_filter = Some(RoomFilterOption::All);
            close_menu = true;
        } else if self.button(cx, ids!(filter_unread_button)).clicked(actions) {
            new_filter = Some(RoomFilterOption::Unread);
            close_menu = true;
        } else if self.button(cx, ids!(filter_favorites_button)).clicked(actions) {
            new_filter = Some(RoomFilterOption::Favorites);
            close_menu = true;
        } else if self.button(cx, ids!(filter_people_button)).clicked(actions) {
            new_filter = Some(RoomFilterOption::People);
            close_menu = true;
        }
        // Sort buttons
        else if self.button(cx, ids!(sort_activity_button)).clicked(actions) {
            new_sort = Some(RoomSortOption::Activity);
            close_menu = true;
        } else if self.button(cx, ids!(sort_alphabetical_button)).clicked(actions) {
            new_sort = Some(RoomSortOption::Alphabetical);
            close_menu = true;
        } else if self.button(cx, ids!(sort_unread_button)).clicked(actions) {
            new_sort = Some(RoomSortOption::Unread);
            close_menu = true;
        }
        // Action buttons
        else if self.button(cx, ids!(mark_all_read_button)).clicked(actions) {
            cx.action(RoomsListHeaderDropdownAction::MarkAllAsRead);
            close_menu = true;
        }

        if let Some(filter) = new_filter {
            cx.action(RoomsListHeaderAction::SetFilter(filter));
            cx.action(RoomsListHeaderDropdownAction::FilterChanged(filter));
        }
        if let Some(sort) = new_sort {
            cx.action(RoomsListHeaderAction::SetSort(sort));
            cx.action(RoomsListHeaderDropdownAction::SortChanged(sort));
        }

        if close_menu {
            self.close(cx);
        }
    }
}

impl RoomsListHeaderDropdown {
    pub fn is_currently_shown(&self, _cx: &mut Cx) -> bool {
        self.visible
    }

    pub fn show(&mut self, cx: &mut Cx, _pos: DVec2, filter: RoomFilterOption, sort: RoomSortOption) {
        self.state = Some(RoomsListHeaderDropdownState { filter, sort });
        self.focused_index = 0;
        self.reset_button_hovers(cx);
        self.visible = true;
        cx.set_key_focus(self.view.area());
        self.redraw(cx);
    }

    /// Activates the currently focused menu item.
    fn activate_focused_item(&mut self, cx: &mut Cx) {
        let action: Option<Box<dyn std::any::Any>> = match self.focused_index {
            0 => Some(Box::new(RoomsListHeaderAction::SetFilter(RoomFilterOption::All))),
            1 => Some(Box::new(RoomsListHeaderAction::SetFilter(RoomFilterOption::Unread))),
            2 => Some(Box::new(RoomsListHeaderAction::SetFilter(RoomFilterOption::Favorites))),
            3 => Some(Box::new(RoomsListHeaderAction::SetFilter(RoomFilterOption::People))),
            4 => Some(Box::new(RoomsListHeaderAction::SetSort(RoomSortOption::Activity))),
            5 => Some(Box::new(RoomsListHeaderAction::SetSort(RoomSortOption::Alphabetical))),
            6 => Some(Box::new(RoomsListHeaderAction::SetSort(RoomSortOption::Unread))),
            7 => {
                cx.action(RoomsListHeaderDropdownAction::MarkAllAsRead);
                self.close(cx);
                return;
            }
            _ => None,
        };

        if let Some(action) = action {
            if let Some(filter) = action.downcast_ref::<RoomsListHeaderAction>() {
                match filter {
                    RoomsListHeaderAction::SetFilter(f) => {
                        cx.action(RoomsListHeaderAction::SetFilter(*f));
                        cx.action(RoomsListHeaderDropdownAction::FilterChanged(*f));
                    }
                    RoomsListHeaderAction::SetSort(s) => {
                        cx.action(RoomsListHeaderAction::SetSort(*s));
                        cx.action(RoomsListHeaderDropdownAction::SortChanged(*s));
                    }
                    _ => {}
                }
            }
            self.close(cx);
        }
    }

    fn reset_button_hovers(&mut self, cx: &mut Cx) {
        // Reset all filter button hovers
        self.button(cx, ids!(filter_all_button)).reset_hover(cx);
        self.button(cx, ids!(filter_unread_button)).reset_hover(cx);
        self.button(cx, ids!(filter_favorites_button)).reset_hover(cx);
        self.button(cx, ids!(filter_people_button)).reset_hover(cx);

        // Reset all sort button hovers
        self.button(cx, ids!(sort_activity_button)).reset_hover(cx);
        self.button(cx, ids!(sort_alphabetical_button)).reset_hover(cx);
        self.button(cx, ids!(sort_unread_button)).reset_hover(cx);

        // Reset action button hovers
        self.button(cx, ids!(mark_all_read_button)).reset_hover(cx);
    }

    fn close(&mut self, cx: &mut Cx) {
        self.visible = false;
        self.state = None;
        cx.revert_key_focus();
        self.redraw(cx);
    }
}

impl RoomsListHeaderDropdownRef {
    pub fn is_currently_shown(&self, cx: &mut Cx) -> bool {
        let Some(inner) = self.borrow() else { return false };
        inner.is_currently_shown(cx)
    }

    pub fn show(&self, cx: &mut Cx, pos: DVec2, filter: RoomFilterOption, sort: RoomSortOption) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.show(cx, pos, filter, sort);
    }
}

/// Actions emitted from the RoomsListHeaderDropdown widget.
#[derive(Clone, Debug)]
pub enum RoomsListHeaderDropdownAction {
    FilterChanged(RoomFilterOption),
    SortChanged(RoomSortOption),
    MarkAllAsRead,
}
