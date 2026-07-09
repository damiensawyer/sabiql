use std::time::Instant;

use crate::cmd::effect::Effect;
use crate::model::app_state::AppState;
use crate::model::shared::input_mode::InputMode;
use crate::ports::outbound::AppSettings;
use crate::update::action::{Action, InputTarget, ModalKind};
use crate::update::dispatch_result::DispatchResult;
use crate::model::browse::query_execution::DEFAULT_PAGE_SIZE;

pub(super) fn reduce_settings(
    state: &mut AppState,
    action: &Action,
    now: Instant,
) -> DispatchResult {
    match action {
        Action::OpenModal(ModalKind::Settings) => {
            state.settings.open(state.ui.theme_id());
            state.modal.set_mode(InputMode::Settings);
            DispatchResult::handled()
        }
        Action::SettingsSelectNext => {
            state.settings.select_next();
            DispatchResult::handled()
        }
        Action::SettingsSelectPrevious => {
            state.settings.select_previous();
            DispatchResult::handled()
        }
        Action::SettingsNextSection => {
            state.settings.switch_next_section();
            DispatchResult::handled()
        }
        Action::SettingsPreviousSection => {
            state.settings.switch_previous_section();
            DispatchResult::handled()
        }
        Action::SettingsStartCustomBrowserEdit => {
            state.settings.start_custom_browser_edit();
            DispatchResult::handled()
        }
        Action::SettingsStopCustomBrowserEdit => {
            state.settings.stop_custom_browser_edit();
            DispatchResult::handled()
        }
        Action::TextInput {
            target: InputTarget::SettingsErBrowser,
            ch,
        } => {
            state.settings.input_custom_browser(*ch);
            DispatchResult::handled()
        }
        Action::TextBackspace {
            target: InputTarget::SettingsErBrowser,
        } => {
            state.settings.backspace_custom_browser();
            DispatchResult::handled()
        }
        Action::TextDelete {
            target: InputTarget::SettingsErBrowser,
        } => {
            state.settings.delete_custom_browser();
            DispatchResult::handled()
        }
        Action::TextMoveCursor {
            target: InputTarget::SettingsErBrowser,
            direction,
        } => {
            state.settings.move_custom_browser_cursor(*direction);
            DispatchResult::handled()
        }
        Action::SettingsSelectRow => {
            state.settings.increment_row_count();
            DispatchResult::handled()
        }
        Action::SettingsDeselectRow => {
            state.settings.decrement_row_count();
            DispatchResult::handled()
        }
        Action::SettingsSelectAllRows => {
            state.settings.set_default_row_count(10_000_000);
            DispatchResult::handled()
        }
        Action::SettingsTogglePagedMode => {
            let current_paged_mode = state.settings.is_paged_mode();
            let next_paged_mode = !current_paged_mode;
            
            if current_paged_mode {
                // If switching from paged mode to row count mode, reset to default
                state.settings.set_default_row_count(DEFAULT_PAGE_SIZE as u32);
            }
            // else: keep the current row count setting
            
            state.settings.set_paged_mode(next_paged_mode);
            DispatchResult::handled()
        }
        Action::SettingsApply => {
            let theme_id = state.settings.selected_theme();
            let settings = AppSettings {
                theme_id,
                keymap_preset: state.settings.selected_keymap_preset(),
                er_browser: state.settings.selected_er_browser(),
                default_row_count: state.settings.selected_default_row_count(),
                paged_mode: state.settings.is_paged_mode(),
            };
            DispatchResult::handled_with(vec![Effect::SaveSettings { settings }])
        }
        Action::SettingsCancel | Action::CloseModal(ModalKind::Settings) => {
            state.settings.discard_selection();
            state.modal.set_mode(InputMode::Normal);
            DispatchResult::handled()
        }
        Action::SettingsSaved(settings) => {
            state.ui.set_theme(settings.theme_id);
            state.settings.commit_saved(
                settings.theme_id,
                settings.keymap_preset,
                settings.er_browser.clone(),
                state.settings.selected_default_row_count(),
                state.settings.is_paged_mode(),
            );
            state
                .messages
                .set_success_at("Settings saved".to_string(), now);
            DispatchResult::handled()
        }
        Action::SettingsSaveFailed(error) => {
            state
                .messages
                .set_error_at(format!("Failed to save settings: {error}"), now);
            DispatchResult::handled()
        }
        _ => DispatchResult::pass(),
    }
}
