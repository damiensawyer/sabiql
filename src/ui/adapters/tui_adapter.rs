use std::time::Instant;

use crossterm::cursor::SetCursorStyle;
use ratatui::widgets::Clear;
use crossterm::execute;

use crate::app::model::app_state::AppState;
use crate::app::model::shared::input_mode::InputMode;
use crate::app::ports::outbound::renderer::{RenderOutput, RenderResult, Renderer};
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
        self.tui.exit()?;
        Ok(())
    }

    fn resume(&mut self) -> RenderResult<()> {
        self.tui.enter()?;
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

fn uses_insert_cursor(state: &AppState) -> bool {
    match state.input_mode() {
        InputMode::JsonbEdit => true,
        InputMode::JsonbDetail => state.jsonb_detail.search().active,
        InputMode::SqlModal => matches!(
            state.sql_modal.status(),
            crate::app::model::sql_editor::modal::SqlModalStatus::Editing
        ),
        _ => false,
    }
}
