//! Collapsible small state event grouping functionality for room timeline.
//! 
//! This module handles the grouping and collapsing of small state events in the room timeline,
//! including header insertion, state management, and event classification.

use std::{collections::HashMap, sync::Arc};
use imbl::Vector;
use makepad_widgets::*;
use matrix_sdk_ui::timeline::{TimelineItem, TimelineItemKind, TimelineItemContent, MsgLikeKind};
use crate::shared::{
    collapsible_header::HeaderCategory,
    collapsible_header_small_state::{CollapsibleHeaderSmallStateAction, CollapsibleHeaderSmallStateWidgetRefExt},
};

/// Manages the state and rendering of collapsible small state event groups
pub struct CollapsibleSmallStateManager {
    /// Map of small state event group IDs to their expanded/collapsed state.
    /// Key is the timeline index where the group starts, value is whether it's expanded.
    pub group_states: HashMap<usize, bool>,
}

impl Default for CollapsibleSmallStateManager {
    fn default() -> Self {
        Self {
            group_states: HashMap::new(),
        }
    }
}

impl CollapsibleSmallStateManager {
    /// Handle collapsible header toggle actions
    pub fn handle_action(&mut self, action: CollapsibleHeaderSmallStateAction, _cx: &mut Cx) -> bool {
        if let CollapsibleHeaderSmallStateAction::Toggled { category, group_id } = action {
            if category == HeaderCategory::SmallStateEvents {
                // Toggle the state for this specific group
                let current_state = self.group_states.get(&group_id).copied().unwrap_or(false);
                self.group_states.insert(group_id, !current_state);
                return true; // Indicates that a redraw is needed
            }
        }
        false
    }

    /// Check if we should insert a collapsible header for small state events
    pub fn should_insert_header(
        &self,
        timeline_item: &TimelineItem,
        prev_item: Option<&TimelineItem>,
    ) -> bool {
        should_insert_small_state_header(timeline_item, prev_item)
    }

    /// Get the group size for a small state event group starting at the given index
    pub fn get_group_size(&self, tl_items: &Vector<Arc<TimelineItem>>, start_idx: usize) -> usize {
        count_small_state_group_size(tl_items, start_idx)
    }

    /// Check if a group is expanded (defaults to false/collapsed)
    pub fn is_group_expanded(&self, group_id: usize) -> bool {
        self.group_states.get(&group_id).copied().unwrap_or(false)
    }

    /// Check if an item should be hidden due to being in a collapsed group
    pub fn should_hide_item(
        &self,
        timeline_item: &TimelineItem,
        tl_items: &Vector<Arc<TimelineItem>>,
        tl_idx: usize,
    ) -> bool {
        if is_small_state_event(timeline_item) {
            if let Some(group_start) = find_group_start_for_item(tl_items, tl_idx) {
                let group_size = count_small_state_group_size(tl_items, group_start);
                if group_size > 3 {
                    let is_group_expanded = self.group_states.get(&group_start).copied().unwrap_or(false);
                    !is_group_expanded
                } else {
                    false // Small group, don't hide
                }
            } else {
                false // Not part of a group, don't hide
            }
        } else {
            false // Not a small state event, don't hide
        }
    }

    /// Create a collapsible header widget for a small state group
    pub fn create_header_widget(
        &self,
        cx: &mut Cx,
        list: &mut PortalList,
        item_id: usize,
        group_id: usize,
    ) -> WidgetRef {
        let is_group_expanded = self.is_group_expanded(group_id);
        
        let header_item = list.item(cx, item_id, id!(CollapsibleHeaderSmallState));
        header_item.as_collapsible_header_small_state().set_details(
            cx,
            is_group_expanded,
            HeaderCategory::SmallStateEvents,
            group_id,
            0, // No unread badge for now
        );
        header_item
    }
}

/// Returns true if the timeline item is a small state event that should be grouped.
pub fn is_small_state_event(timeline_item: &TimelineItem) -> bool {
    match timeline_item.kind() {
        TimelineItemKind::Event(event_tl_item) => match event_tl_item.content() {
            TimelineItemContent::MsgLike(msg_like_content) => matches!(
                msg_like_content.kind,
                MsgLikeKind::Poll(_) | MsgLikeKind::Redacted | MsgLikeKind::UnableToDecrypt(_) | MsgLikeKind::Other(_)
            ),
            TimelineItemContent::MembershipChange(_) |
            TimelineItemContent::ProfileChange(_) |
            TimelineItemContent::OtherState(_) => true,
            _ => false,
        },
        _ => false,
    }
}

/// Returns true if we should insert a collapsible header before this item.
/// This happens when:
/// 1. The current item is a small state event
/// 2. The previous item was not a small state event (or doesn't exist)
pub fn should_insert_small_state_header(
    current_item: &TimelineItem,
    prev_item: Option<&TimelineItem>,
) -> bool {
    if !is_small_state_event(current_item) {
        return false;
    }
    
    // Insert header if this is the first item or previous item wasn't a small state event
    prev_item.map_or(true, |prev| !is_small_state_event(prev))
}

/// Finds the group start index for a given timeline item.
/// Returns the timeline index where the small state group containing this item starts.
pub fn find_group_start_for_item(tl_items: &Vector<Arc<TimelineItem>>, item_idx: usize) -> Option<usize> {
    let current_item = tl_items.get(item_idx)?;
    if !is_small_state_event(current_item) {
        return None;
    }
    
    // Search backwards to find where this group starts
    for i in (0..=item_idx).rev() {
        let item = tl_items.get(i)?;
        let prev_item = if i > 0 { tl_items.get(i - 1) } else { None };
        
        if should_insert_small_state_header(item, prev_item.map(|i| i.as_ref())) {
            return Some(i);
        }
    }
    
    None
}

/// Counts the number of consecutive small state events starting from the given index.
/// Returns the size of the small state event group.
pub fn count_small_state_group_size(tl_items: &Vector<Arc<TimelineItem>>, start_idx: usize) -> usize {
    let mut count = 0;
    for i in start_idx..tl_items.len() {
        if let Some(item) = tl_items.get(i) {
            if is_small_state_event(item) {
                count += 1;
            } else {
                break; // End of the group
            }
        } else {
            break;
        }
    }
    count
}