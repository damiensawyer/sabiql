use std::time::Instant;

use color_eyre::eyre::Report;
use crossterm::cursor::SetCursorStyle;
use crossterm::execute;
use ratatui::widgets::Clear;

use crate::app::model::app_state::AppState;
use crate::app::model::browse::json_detail::JsonDetailMode;
use crate::app::model::shared::help::HelpMode;
use crate::app::model::shared::input_mode::InputMode;
use crate::app::model::sql_editor::modal::SqlModalStatus;
use crate::app::ports::outbound::renderer::{RenderError, RenderOutput, RenderResult, Renderer};
use crate::app::services::AppServices;
use crate::shell::layout::MainLayout;
use crate::tui::TuiRunner;

pub struct TuiAdapter<'a> {
    tui: &'a mut TuiRunner,
    last_cursor_insert: Option<bool>,
}

impl<'a> TuiAdapter<'a> {
    pub fn new(tui: &'a mut TuiRunner) -> Self {
        Self {
            tui,
            last_cursor_insert: None,
        }
    }
}

impl Renderer for TuiAdapter<'_> {
    fn draw(
        &mut self,
        state: &AppState,
        services: &AppServices,
        now: Instant,
    ) -> RenderResult<RenderOutput> {
        let mut output = RenderOutput::default();
        self.tui.terminal().draw(|frame| {
            // Clear the entire screen before rendering. This is necessary
            // because after leaving and re-entering the alternate screen
            // buffer (e.g. when an external editor exits), the buffer may
            // contain stale content that widgets don't overwrite cleanly.
            frame.render_widget(Clear, frame.area());
            output = MainLayout::render(frame, state, None, services, now);
        })?;
        let uses_insert = uses_insert_cursor(state);
        if self.last_cursor_insert != Some(uses_insert) {
            execute!(
                std::io::stdout(),
                if uses_insert {
                    SetCursorStyle::SteadyBar
                } else {
                    SetCursorStyle::SteadyBlock
                }
            )?;
            self.last_cursor_insert = Some(uses_insert);
        }
        Ok(output)
    }

    fn suspend(&mut self) -> RenderResult<()> {
        // Leaves raw mode, the alternate screen, and stops the background
        // input-reader task so a spawned child process (e.g. $EDITOR) has
        // exclusive use of the terminal and stdin.
        self.tui.exit().map_err(terminal_error)?;
        Ok(())
    }

    fn resume(&mut self) -> RenderResult<()> {
        self.tui.enter().map_err(terminal_error)?;
        // The physical screen was handed to another process while
        // suspended, so ratatui's cached "previous frame" no longer
        // reflects what's actually on screen. Clearing resets that cache
        // and forces the next draw() to repaint every cell instead of
        // diffing against stale state, which otherwise left the screen
        // blank after the editor exited.
        self.tui.terminal().clear()?;
        self.last_cursor_insert = None;
        Ok(())
    }
}

fn terminal_error(error: Report) -> RenderError {
    RenderError::from(std::io::Error::other(error.to_string()))
}

fn uses_insert_cursor(state: &AppState) -> bool {
    match state.input_mode() {
        InputMode::JsonEdit => true,
        InputMode::JsonDetail => matches!(state.json_detail.mode(), JsonDetailMode::Searching),
        InputMode::CellDetail => state.cell_detail.search().is_active(),
        InputMode::Help => matches!(state.ui.help().mode(), HelpMode::EditingFilter),
        InputMode::SqlModal => matches!(state.sql_modal.status(), SqlModalStatus::Editing),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_filter_uses_insert_cursor() {
        let mut state = AppState::new("test".to_string());
        state.modal.set_mode(InputMode::Help);

        assert!(!uses_insert_cursor(&state));

        state.ui.help_mut().enter_filter_editing();

        assert!(uses_insert_cursor(&state));
    }
}
