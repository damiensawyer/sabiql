use std::time::{Duration, Instant};

use crate::cmd::effect::Effect;
use crate::model::app_state::AppState;
use crate::model::shared::input_mode::InputMode;
use crate::model::shared::key_sequence::KeySequenceState;
use crate::model::sql_editor::modal::{SqlModalStatus, sql_modal_visible_rows};
use crate::update::action::{Action, CursorMove, InputTarget};
use crate::update::dispatch_result::DispatchResult;

pub(super) fn reduce_editing(
    state: &mut AppState,
    action: &Action,
    now: Instant,
) -> DispatchResult {
    match action {
        // Clipboard paste
        Action::Paste(text) if state.modal.active_mode() == InputMode::SqlModal => {
            if !matches!(state.sql_modal.status(), SqlModalStatus::Editing) {
                return DispatchResult::handled();
            }
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            state.sql_modal.editor.insert_str(&normalized);
            state
                .sql_modal
                .editor
                .update_scroll(sql_modal_visible_rows(state.ui.terminal_height));
            state
                .sql_modal
                .schedule_completion_after_dismiss(now + Duration::from_millis(100));
            state.sql_modal.enter_editing();
            DispatchResult::handled()
        }

        // Text editing
        Action::TextInput {
            target: InputTarget::SqlModal,
            ch: c,
        } => {
            state.sql_modal.enter_editing();
            state.sql_modal.editor.insert_char(*c);
            state
                .sql_modal
                .editor
                .update_scroll(sql_modal_visible_rows(state.ui.terminal_height));
            state
                .sql_modal
                .schedule_completion(now + Duration::from_millis(100));
            DispatchResult::handled()
        }
        Action::TextBackspace {
            target: InputTarget::SqlModal,
        } => {
            state.sql_modal.enter_editing();
            state.sql_modal.editor.backspace();
            state
                .sql_modal
                .editor
                .update_scroll(sql_modal_visible_rows(state.ui.terminal_height));
            state
                .sql_modal
                .schedule_completion(now + Duration::from_millis(100));
            DispatchResult::handled()
        }
        Action::TextDelete {
            target: InputTarget::SqlModal,
        } => {
            state.sql_modal.enter_editing();
            state.sql_modal.editor.delete();
            state
                .sql_modal
                .editor
                .update_scroll(sql_modal_visible_rows(state.ui.terminal_height));
            state
                .sql_modal
                .schedule_completion(now + Duration::from_millis(100));
            DispatchResult::handled()
        }
        Action::SqlModalNewLine => {
            state.sql_modal.enter_editing();
            state.sql_modal.editor.insert_newline();
            state
                .sql_modal
                .editor
                .update_scroll(sql_modal_visible_rows(state.ui.terminal_height));
            state
                .sql_modal
                .schedule_completion(now + Duration::from_millis(100));
            DispatchResult::handled()
        }
        Action::SqlModalTab => {
            state.sql_modal.enter_editing();
            state.sql_modal.editor.insert_tab();
            state
                .sql_modal
                .editor
                .update_scroll(sql_modal_visible_rows(state.ui.terminal_height));
            state
                .sql_modal
                .schedule_completion(now + Duration::from_millis(100));
            DispatchResult::handled()
        }
        Action::TextMoveCursor {
            target: InputTarget::SqlModal,
            direction: movement,
        } => {
            match movement {
                CursorMove::ViewportTop
                | CursorMove::ViewportMiddle
                | CursorMove::ViewportBottom => {
                    state.sql_modal.editor.move_cursor_to_viewport_position(
                        *movement,
                        sql_modal_visible_rows(state.ui.terminal_height),
                    );
                }
                _ => state.sql_modal.editor.move_cursor(*movement),
            }
            state
                .sql_modal
                .editor
                .update_scroll(sql_modal_visible_rows(state.ui.terminal_height));
            state.ui.key_sequence = KeySequenceState::Idle;
            DispatchResult::handled()
        }
        Action::SqlModalClear => {
            state.sql_modal.editor.clear();
            state.sql_modal.reset_completion();
            state.ui.key_sequence = KeySequenceState::Idle;
            DispatchResult::handled()
        }

        // External editor: launch the user's $EDITOR with the current SQL as a temp file
        Action::ExternalEditorOpen { .. } if state.modal.active_mode() == InputMode::SqlModal => {
            state.sql_modal.dismiss_completion();
            // Push `Normal` mode so the modal is hidden during the blocking
            // editor session. `spawn_blocking` returns immediately; without
            // this the main loop's next `draw()` could render the modal on
            // top of the editor before `run_sync` calls `LeaveAlternateScreen`.
            state.modal.push_mode(InputMode::Normal);
            // Create a unique temp file for this edit session
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros();
            let file = std::env::temp_dir()
                .join(format!("sabiql-editor-{timestamp}.sql"));
            DispatchResult::handled_with(vec![Effect::LaunchExternalEditor {
                file,
            }])
        }

        // External editor: received updated content from the editor
        Action::ExternalEditorUpdated { content } => {
            state.modal.pop_mode();
            state.sql_modal.load_query_for_editing(content.clone());
            // load_query_for_editing's set_content() puts the cursor at the
            // end of the new text but leaves scroll_row at 0, so a
            // multi-line query would leave the cursor scrolled out of view.
            state
                .sql_modal
                .editor
                .update_scroll(sql_modal_visible_rows(state.ui.terminal_height));
            // Mark render-dirty so the main loop injects a Render effect,
            // ensuring the screen is properly redrawn after the editor exits.
            state.render_dirty = true;
            DispatchResult::handled()
        }

        // External editor: the editor failed to launch or exited non-zero
        Action::ExternalEditorFailed(message) => {
            state.modal.pop_mode();
            state.messages.set_error_at(message.clone(), now);
            state.render_dirty = true;
            DispatchResult::handled()
        }

        _ => DispatchResult::pass(),
    }
}
