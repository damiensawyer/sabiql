use crate::app::model::app_state::AppState;
use crate::app::model::shared::settings::{ErBrowserChoice, SettingsSection};
use crate::app::update::input::keybindings::settings;
use crate::primitives::molecules::{FooterHintBar, HintTuple};

const EDIT_DONE_HINT: HintTuple = ("Esc", "Done", false);
const EDIT_TYPE_HINT: HintTuple = ("Type", "Browser", false);

pub fn settings_hints(state: &AppState) -> Vec<HintTuple> {
    if state.settings.is_editing_custom_er_browser() {
        return vec![settings::APPLY.as_hint(), EDIT_DONE_HINT, EDIT_TYPE_HINT];
    }

    let mut hints = vec![settings::APPLY.as_hint(), settings::SELECT.as_hint()];
    if state.settings.section() == SettingsSection::ErDiagram
        && state.settings.selected_er_browser_choice() == ErBrowserChoice::Custom
    {
        hints.push(settings::EDIT.as_hint());
    }
    if state.settings.section() == SettingsSection::LowScroll {
        hints.push(settings::TOGGLE.as_hint());
    }
    hints.push(settings::SECTION.as_hint());
    hints.push(settings::CANCEL.as_hint());
    hints
}

pub fn settings_modal_hint_bar(state: &AppState) -> FooterHintBar {
    FooterHintBar::new(settings_hints(state)).without_item(settings::SELECT.key_short)
}
