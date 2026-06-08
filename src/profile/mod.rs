use makepad_widgets::ScriptVm;

pub mod moderation_action_modal;
pub mod user_profile;
pub mod user_profile_cache;

pub fn script_mod(vm: &mut ScriptVm) {
    user_profile::script_mod(vm);
    moderation_action_modal::script_mod(vm);
}
