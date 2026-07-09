use crate::model::app_state::AppState;
use crate::model::shared::key_sequence::KeySequenceState;
use crate::model::shared::viewport::{calculate_next_column_offset, calculate_prev_column_offset};
use crate::update::action::{
    Action, CursorPosition, ScrollAmount, ScrollDirection, ScrollTarget, ScrollToCursorTarget,
};
use crate::update::dispatch_result::DispatchResult;
use crate::update::browse::query::preview_effect_for_current_table;

pub(super) fn result_row_count(state: &AppState) -> usize {
    state.query.visible_result().map_or(0, |r| r.rows.len())
}

pub(super) fn result_col_count(state: &AppState) -> usize {
    state.query.visible_result().map_or(0, |r| r.columns.len())
}

pub(super) fn result_max_scroll(state: &AppState) -> usize {
    let visible = state.result_visible_rows();
    result_row_count(state).saturating_sub(visible)
}

/// Check if we're at the end of the visible results and can paginate further
pub(super) fn can_paginate_at_end(state: &AppState) -> bool {
    if let Some(result) = state.query.visible_result() {
        if !state.query.can_paginate_visible_result() {
            return false;
        }
        let current_row_count = result.rows.len();
        if current_row_count == 0 {
            return false;
        }
        let max_scroll = result_max_scroll(state);
        // We're at the bottom if scroll_offset equals max_scroll
        if state.result_interaction.scroll_offset != max_scroll {
            return false;
        }
        // There are more pages if pagination can continue
        state.query.pagination.can_next()
    } else {
        false
    }
}

/// Trigger automatic pagination when at the end of results
/// Returns Some(effects) if pagination is triggered, None otherwise
pub(super) fn trigger_automatic_pagination(state: &mut AppState) -> Option<Vec<crate::cmd::effect::Effect>> {
    if can_paginate_at_end(state) {
        let next_page = state.query.pagination.current_page + 1;
        let generation = state.session.selection_generation();
        let page_size = state.settings.selected_default_row_count() as usize;
        match preview_effect_for_current_table(state, std::time::Instant::now(), next_page, generation, page_size) {
            Some(effect) => {
                state.result_interaction.reset_view();
                Some(vec![effect])
            }
            None => None,
        }
    } else {
        None
    }
}

fn ensure_row_visible(state: &mut AppState) {
    if let Some(row) = state.result_interaction.selection().row() {
        let visible = state.result_visible_rows();
        if visible == 0 {
            return;
        }
        if row < state.result_interaction.scroll_offset {
            state.result_interaction.scroll_offset = row;
        } else if row >= state.result_interaction.scroll_offset + visible {
            state.result_interaction.scroll_offset = row - visible + 1;
        }
    }
}

fn move_row_or_scroll(state: &mut AppState, new_row: usize, scroll_fn: impl FnOnce(&mut AppState)) {
    if state.result_interaction.selection().row().is_some() {
        state.result_interaction.move_row(new_row);
        ensure_row_visible(state);
    } else {
        scroll_fn(state);
    }
}

fn page_scroll_delta(state: &AppState, amount: ScrollAmount) -> Option<usize> {
    amount.page_delta(state.result_visible_rows())
}

fn scroll_result_by(state: &mut AppState, direction: ScrollDirection, delta: usize) {
    let max_scroll = result_max_scroll(state);
    state.result_interaction.scroll_offset =
        direction.clamp_vertical_offset(state.result_interaction.scroll_offset, max_scroll, delta);
}

fn move_result_row_and_scroll(state: &mut AppState, direction: ScrollDirection, delta: usize) {
    let max_row = result_row_count(state).saturating_sub(1);
    if let Some(row) = state.result_interaction.selection().row() {
        let new_row = match direction {
            ScrollDirection::Down => (row + delta).min(max_row),
            ScrollDirection::Up => row.saturating_sub(delta),
            _ => row,
        };
        state.result_interaction.move_row(new_row);
    }
    scroll_result_by(state, direction, delta);
}

pub fn reduce_scroll(state: &mut AppState, action: &Action) -> DispatchResult {
    match action {
        Action::Scroll {
            target: ScrollTarget::Result,
            direction: ScrollDirection::Up,
            amount: ScrollAmount::Line,
        } => {
            let new_row = state
                .result_interaction
                .selection()
                .row()
                .and_then(|r| r.checked_sub(1));
            match new_row {
                Some(r) => move_row_or_scroll(state, r, |_| {}),
                None if state.result_interaction.selection().row().is_none() => {
                    state.result_interaction.scroll_offset =
                        state.result_interaction.scroll_offset.saturating_sub(1);
                }
                _ => {} // row == 0, no-op
            }
            DispatchResult::handled()
        }
        Action::Scroll {
            target: ScrollTarget::Result,
            direction: ScrollDirection::Down,
            amount: ScrollAmount::Line,
        } => {
            let max_row = result_row_count(state).saturating_sub(1);
            let new_row = state
                .result_interaction
                .selection()
                .row()
                .map_or(0, |r| (r + 1).min(max_row));
            move_row_or_scroll(state, new_row, |s| {
                let max_scroll = result_max_scroll(s);
                if s.result_interaction.scroll_offset < max_scroll {
                    s.result_interaction.scroll_offset += 1;
                }
            });
            // Trigger pagination if we're now at the bottom and can paginate further
            if let Some(effects) = trigger_automatic_pagination(state) {
                return DispatchResult::handled_with(effects);
            }
            DispatchResult::handled()
        }
        Action::Scroll {
            target: ScrollTarget::Result,
            direction: ScrollDirection::Up,
            amount: ScrollAmount::ToStart,
        } => {
            move_row_or_scroll(state, 0, |s| s.result_interaction.scroll_offset = 0);
            DispatchResult::handled()
        }
        Action::Scroll {
            target: ScrollTarget::Result,
            direction: ScrollDirection::Down,
            amount: ScrollAmount::ToEnd,
        } => {
            let max_row = result_row_count(state).saturating_sub(1);
            let max_scroll = result_max_scroll(state);
            move_row_or_scroll(state, max_row, |s| {
                s.result_interaction.scroll_offset = max_scroll;
            });
            // Automatically load next page if at end and can paginate
            if let Some(effects) = trigger_automatic_pagination(state) {
                return DispatchResult::handled_with(effects);
            }
            DispatchResult::handled()
        }
        Action::Scroll {
            target: ScrollTarget::Result,
            direction: ScrollDirection::Up,
            amount: ScrollAmount::ViewportMiddle,
        } => {
            if state.result_interaction.selection().row().is_some() {
                let visible = state.result_visible_rows();
                let total = result_row_count(state);
                let offset = state.result_interaction.scroll_offset;
                let displayed = visible.min(total.saturating_sub(offset));
                let target_row = offset + displayed / 2;
                state.result_interaction.move_row(target_row);
                ensure_row_visible(state);
            }
            DispatchResult::handled()
        }
        Action::Scroll {
            target: ScrollTarget::Result,
            direction: ScrollDirection::Up,
            amount: ScrollAmount::ViewportTop,
        } => {
            if state.result_interaction.selection().row().is_some() {
                let target = state.result_interaction.scroll_offset;
                state.result_interaction.move_row(target);
                ensure_row_visible(state);
            }
            DispatchResult::handled()
        }
        Action::Scroll {
            target: ScrollTarget::Result,
            direction: ScrollDirection::Down,
            amount: ScrollAmount::ViewportBottom,
        } => {
            if state.result_interaction.selection().row().is_some() {
                let visible = state.result_visible_rows();
                let total = result_row_count(state);
                let offset = state.result_interaction.scroll_offset;
                let displayed = visible.min(total.saturating_sub(offset));
                let target = offset + displayed.saturating_sub(1);
                state.result_interaction.move_row(target);
                ensure_row_visible(state);
            }
            DispatchResult::handled()
        }
        Action::Scroll {
            target: ScrollTarget::Result,
            direction: direction @ (ScrollDirection::Down | ScrollDirection::Up),
            amount: amount @ (ScrollAmount::HalfPage | ScrollAmount::FullPage),
        } => {
            let delta = page_scroll_delta(state, *amount).unwrap_or(0);
            if delta > 0 {
                move_result_row_and_scroll(state, *direction, delta);
            }
            // Trigger pagination if scrolling down and we're at the bottom
            if *direction == ScrollDirection::Down {
                if let Some(effects) = trigger_automatic_pagination(state) {
                    return DispatchResult::handled_with(effects);
                }
            }
            DispatchResult::handled()
        }
        // Scroll-to-cursor (zz/zt/zb): only meaningful with an active cell
        Action::ScrollToCursor {
            target: ScrollToCursorTarget::Result,
            position: CursorPosition::Center,
        } => {
            state.ui.key_sequence = KeySequenceState::Idle;
            if let Some(row) = state.result_interaction.selection().row() {
                let visible = state.result_visible_rows();
                if visible > 0 {
                    let max_scroll = result_max_scroll(state);
                    state.result_interaction.scroll_offset =
                        row.saturating_sub(visible / 2).min(max_scroll);
                }
            }
            DispatchResult::handled()
        }
        Action::ScrollToCursor {
            target: ScrollToCursorTarget::Result,
            position: CursorPosition::Top,
        } => {
            state.ui.key_sequence = KeySequenceState::Idle;
            if let Some(row) = state.result_interaction.selection().row() {
                let visible = state.result_visible_rows();
                if visible > 0 {
                    let max_scroll = result_max_scroll(state);
                    state.result_interaction.scroll_offset = row.min(max_scroll);
                }
            }
            DispatchResult::handled()
        }
        Action::ScrollToCursor {
            target: ScrollToCursorTarget::Result,
            position: CursorPosition::Bottom,
        } => {
            state.ui.key_sequence = KeySequenceState::Idle;
            if let Some(row) = state.result_interaction.selection().row() {
                let visible = state.result_visible_rows();
                if visible > 0 {
                    let max_scroll = result_max_scroll(state);
                    state.result_interaction.scroll_offset = row
                        .saturating_sub(visible.saturating_sub(1))
                        .min(max_scroll);
                }
            }
            DispatchResult::handled()
        }
        Action::Scroll {
            target: ScrollTarget::Result,
            direction: ScrollDirection::Left,
            amount: ScrollAmount::Line,
        } => {
            state.result_interaction.horizontal_offset =
                calculate_prev_column_offset(state.result_interaction.horizontal_offset);
            DispatchResult::handled()
        }
        Action::Scroll {
            target: ScrollTarget::Result,
            direction: ScrollDirection::Right,
            amount: ScrollAmount::Line,
        } => {
            state.result_interaction.horizontal_offset = calculate_next_column_offset(
                state.result_interaction.horizontal_offset,
                state.ui.result_viewport_plan.max_offset,
            );
            DispatchResult::handled()
        }
        _ => DispatchResult::pass(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::{QueryResult, QuerySource};
    use crate::model::browse::query_execution::PaginationState;
    use crate::model::shared::key_sequence::Prefix;

    fn state_with_result_rows(rows: usize, pane_height: u16) -> AppState {
        let mut state = AppState::new("test".to_string());
        state.ui.result_pane_height = pane_height;
        let result_rows: Vec<Vec<String>> = (0..rows).map(|i| vec![format!("{}", i)]).collect();
        state
            .query
            .set_current_result(Arc::new(QueryResult::success(
                String::new(),
                vec!["id".to_string()],
                result_rows,
                1,
                QuerySource::Preview,
            )));
        state
    }

    mod page_scroll {
        use super::*;

        mod scroll_mode {
            use super::*;

            #[test]
            fn half_page_down_from_top() {
                let mut state = state_with_result_rows(100, 25);
                // visible = 25 - 5 = 20, half = 10
                let effects = reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert!(effects.is_handled());
                assert_eq!(state.result_interaction.scroll_offset, 10);
            }

            #[test]
            fn half_page_up_from_middle() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.scroll_offset = 50;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Up,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert_eq!(state.result_interaction.scroll_offset, 40);
            }

            #[test]
            fn full_page_down_clamped_at_max() {
                let mut state = state_with_result_rows(30, 25);
                // visible = 20, max_scroll = 30-20 = 10
                state.result_interaction.scroll_offset = 5;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::FullPage,
                    },
                );

                assert_eq!(state.result_interaction.scroll_offset, 10);
            }

            #[test]
            fn full_page_up_clamped_at_zero() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.scroll_offset = 5;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Up,
                        amount: ScrollAmount::FullPage,
                    },
                );

                assert_eq!(state.result_interaction.scroll_offset, 0);
            }
        }

        mod viewport_position {
            use super::*;

            #[test]
            fn scroll_middle_moves_row_to_viewport_center_in_cell_active() {
                let mut state = state_with_result_rows(100, 25);
                // visible = 20, mid_viewport = 10, target = 0 + 10 = 10
                state.result_interaction.activate_cell(0, 0);

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Up,
                        amount: ScrollAmount::ViewportMiddle,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(10));
            }

            #[test]
            fn scroll_middle_is_noop_in_scroll_mode() {
                let mut state = state_with_result_rows(100, 25);

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Up,
                        amount: ScrollAmount::ViewportMiddle,
                    },
                );

                assert_eq!(state.result_interaction.scroll_offset, 0);
            }
        }

        mod cell_active {
            use super::*;

            #[test]
            fn half_page_down_moves_both_cursor_and_viewport() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.activate_cell(10, 0);
                state.result_interaction.scroll_offset = 5;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(20));
                assert_eq!(state.result_interaction.scroll_offset, 15);
            }

            #[test]
            fn half_page_up_moves_both_cursor_and_viewport() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.activate_cell(30, 0);
                state.result_interaction.scroll_offset = 25;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Up,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(20));
                assert_eq!(state.result_interaction.scroll_offset, 15);
            }

            #[test]
            fn half_page_down_preserves_relative_position() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.activate_cell(15, 3);
                state.result_interaction.scroll_offset = 10;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(25));
                assert_eq!(state.result_interaction.selection().cell(), Some(3));
                assert_eq!(state.result_interaction.scroll_offset, 20);
                let relative = state.result_interaction.selection().row().unwrap()
                    - state.result_interaction.scroll_offset;
                assert_eq!(relative, 5);
            }

            #[test]
            fn half_page_down_preserves_column() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.activate_cell(10, 3);
                state.result_interaction.scroll_offset = 5;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(20));
                assert_eq!(state.result_interaction.selection().cell(), Some(3));
                assert_eq!(state.result_interaction.scroll_offset, 15);
            }

            #[test]
            fn full_page_down_moves_both() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.activate_cell(10, 0);
                state.result_interaction.scroll_offset = 5;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::FullPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(30));
                assert_eq!(state.result_interaction.scroll_offset, 25);
            }

            #[test]
            fn full_page_up_moves_both() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.activate_cell(40, 0);
                state.result_interaction.scroll_offset = 30;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Up,
                        amount: ScrollAmount::FullPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(20));
                assert_eq!(state.result_interaction.scroll_offset, 10);
            }
        }

        mod boundary_conditions {
            use super::*;

            #[test]
            fn zero_height_scroll_mode_is_noop() {
                let mut state = state_with_result_rows(100, 0);
                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert_eq!(state.result_interaction.scroll_offset, 0);
            }

            #[test]
            fn half_page_down_clamps_near_bottom() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.activate_cell(95, 0);
                state.result_interaction.scroll_offset = 75;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(99));
                assert_eq!(state.result_interaction.scroll_offset, 80);
            }

            #[test]
            fn half_page_up_clamps_near_top() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.activate_cell(5, 0);
                state.result_interaction.scroll_offset = 3;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Up,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(0));
                assert_eq!(state.result_interaction.scroll_offset, 0);
            }

            #[test]
            fn visible_zero_cell_active_is_noop() {
                let mut state = state_with_result_rows(100, 0);
                state.result_interaction.activate_cell(5, 1);
                state.result_interaction.scroll_offset = 3;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(5));
                assert_eq!(state.result_interaction.selection().cell(), Some(1));
                assert_eq!(state.result_interaction.scroll_offset, 3);
            }

            #[test]
            fn data_fewer_than_viewport_scroll_stays_zero() {
                let mut state = state_with_result_rows(10, 25);
                state.result_interaction.activate_cell(3, 0);

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::HalfPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(9));
                assert_eq!(state.result_interaction.scroll_offset, 0);
            }

            #[test]
            fn full_page_down_clamps_near_bottom() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.activate_cell(90, 0);
                state.result_interaction.scroll_offset = 75;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::FullPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(99));
                assert_eq!(state.result_interaction.scroll_offset, 80);
            }

            #[test]
            fn full_page_up_clamps_near_top() {
                let mut state = state_with_result_rows(100, 25);
                state.result_interaction.activate_cell(10, 0);
                state.result_interaction.scroll_offset = 5;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Up,
                        amount: ScrollAmount::FullPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(0));
                assert_eq!(state.result_interaction.scroll_offset, 0);
            }

            #[test]
            fn visible_zero_cell_active_full_page_is_noop() {
                let mut state = state_with_result_rows(100, 0);
                state.result_interaction.activate_cell(5, 1);
                state.result_interaction.scroll_offset = 3;

                reduce_scroll(
                    &mut state,
                    &Action::Scroll {
                        target: ScrollTarget::Result,
                        direction: ScrollDirection::Down,
                        amount: ScrollAmount::FullPage,
                    },
                );

                assert_eq!(state.result_interaction.selection().row(), Some(5));
                assert_eq!(state.result_interaction.selection().cell(), Some(1));
                assert_eq!(state.result_interaction.scroll_offset, 3);
            }
        }
    }

    mod result_scroll_to_cursor {
        use super::*;

        #[test]
        fn scroll_cursor_center_centers_on_selected_row() {
            let mut state = state_with_result_rows(100, 25);
            // visible = 20
            state.result_interaction.activate_cell(50, 0);
            state.result_interaction.scroll_offset = 50;
            state.ui.key_sequence = KeySequenceState::WaitingSecondKey(Prefix::Z);

            reduce_scroll(
                &mut state,
                &Action::ScrollToCursor {
                    target: ScrollToCursorTarget::Result,
                    position: CursorPosition::Center,
                },
            );

            // row=50, visible=20, offset=50-10=40, max=80 → 40
            assert_eq!(state.result_interaction.scroll_offset, 40);
            assert_eq!(state.ui.key_sequence, KeySequenceState::Idle);
        }

        #[test]
        fn scroll_cursor_top_puts_row_at_top() {
            let mut state = state_with_result_rows(100, 25);
            state.result_interaction.activate_cell(30, 0);
            state.result_interaction.scroll_offset = 20;
            state.ui.key_sequence = KeySequenceState::WaitingSecondKey(Prefix::Z);

            reduce_scroll(
                &mut state,
                &Action::ScrollToCursor {
                    target: ScrollToCursorTarget::Result,
                    position: CursorPosition::Top,
                },
            );

            assert_eq!(state.result_interaction.scroll_offset, 30);
            assert_eq!(state.ui.key_sequence, KeySequenceState::Idle);
        }

        #[test]
        fn scroll_cursor_bottom_puts_row_at_bottom() {
            let mut state = state_with_result_rows(100, 25);
            state.result_interaction.activate_cell(30, 0);
            state.result_interaction.scroll_offset = 30;
            state.ui.key_sequence = KeySequenceState::WaitingSecondKey(Prefix::Z);

            reduce_scroll(
                &mut state,
                &Action::ScrollToCursor {
                    target: ScrollToCursorTarget::Result,
                    position: CursorPosition::Bottom,
                },
            );

            // row=30, visible=20, offset=30-19=11, max=80 → 11
            assert_eq!(state.result_interaction.scroll_offset, 11);
            assert_eq!(state.ui.key_sequence, KeySequenceState::Idle);
        }

        #[test]
        fn scroll_cursor_center_is_noop_in_scroll_mode() {
            let mut state = state_with_result_rows(100, 25);
            state.result_interaction.scroll_offset = 20;
            state.ui.key_sequence = KeySequenceState::WaitingSecondKey(Prefix::Z);

            reduce_scroll(
                &mut state,
                &Action::ScrollToCursor {
                    target: ScrollToCursorTarget::Result,
                    position: CursorPosition::Center,
                },
            );

            // No row selected, offset unchanged
            assert_eq!(state.result_interaction.scroll_offset, 20);
            assert_eq!(state.ui.key_sequence, KeySequenceState::Idle);
        }

        #[test]
        fn scroll_cursor_top_clamps_to_max_scroll() {
            let mut state = state_with_result_rows(100, 25);
            // visible=20, max_scroll=80
            state.result_interaction.activate_cell(95, 0);
            state.result_interaction.scroll_offset = 80;
            state.ui.key_sequence = KeySequenceState::WaitingSecondKey(Prefix::Z);

            reduce_scroll(
                &mut state,
                &Action::ScrollToCursor {
                    target: ScrollToCursorTarget::Result,
                    position: CursorPosition::Top,
                },
            );

            // row=95, clamped to max_scroll=80
            assert_eq!(state.result_interaction.scroll_offset, 80);
            assert_eq!(state.ui.key_sequence, KeySequenceState::Idle);
        }
    }

    mod automatic_pagination {
        use super::*;

        #[test]
        fn end_of_scroll_triggers_next_page() {
            let mut state = state_with_result_rows(100, 25);
            // visible=20, max_scroll=80, scrolled to bottom
            state.result_interaction.scroll_offset = 80;
            // Set up pagination state with more pages available
            state.query.pagination = PaginationState {
                current_page: 0,
                total_rows_estimate: Some(1500),
                reached_end: false,
                schema: "public".to_string(),
                table: "users".to_string(),
            };

            // Scroll to end
            reduce_scroll(
                &mut state,
                &Action::Scroll {
                    target: ScrollTarget::Result,
                    direction: ScrollDirection::Down,
                    amount: ScrollAmount::ToEnd,
                },
            );

            // Should have triggered pagination
            assert_eq!(state.query.pagination.current_page, 1);
        }

        #[test]
        fn line_scroll_at_bottom_triggers_next_page() {
            let mut state = state_with_result_rows(100, 25);
            // visible=20, max_scroll=80, scrolled to bottom
            state.result_interaction.scroll_offset = 80;
            // Set up pagination state with more pages available
            state.query.pagination = PaginationState {
                current_page: 0,
                total_rows_estimate: Some(1500),
                reached_end: false,
                schema: "public".to_string(),
                table: "users".to_string(),
            };

            // Scroll down one line (should trigger pagination)
            reduce_scroll(
                &mut state,
                &Action::Scroll {
                    target: ScrollTarget::Result,
                    direction: ScrollDirection::Down,
                    amount: ScrollAmount::Line,
                },
            );

            // Should have triggered pagination
            assert_eq!(state.query.pagination.current_page, 1);
        }

        #[test]
        fn half_page_scroll_at_bottom_triggers_next_page() {
            let mut state = state_with_result_rows(100, 25);
            // visible=20, max_scroll=80, scrolled to bottom
            state.result_interaction.scroll_offset = 80;
            // Set up pagination state with more pages available
            state.query.pagination = PaginationState {
                current_page: 0,
                total_rows_estimate: Some(1500),
                reached_end: false,
                schema: "public".to_string(),
                table: "users".to_string(),
            };

            // Scroll down half page (should trigger pagination)
            reduce_scroll(
                &mut state,
                &Action::Scroll {
                    target: ScrollTarget::Result,
                    direction: ScrollDirection::Down,
                    amount: ScrollAmount::HalfPage,
                },
            );

            // Should have triggered pagination
            assert_eq!(state.query.pagination.current_page, 1);
        }

        #[test]
        fn full_page_scroll_at_bottom_triggers_next_page() {
            let mut state = state_with_result_rows(100, 25);
            // visible=20, max_scroll=80, scrolled to bottom
            state.result_interaction.scroll_offset = 80;
            // Set up pagination state with more pages available
            state.query.pagination = PaginationState {
                current_page: 0,
                total_rows_estimate: Some(1500),
                reached_end: false,
                schema: "public".to_string(),
                table: "users".to_string(),
            };

            // Scroll down full page (should trigger pagination)
            reduce_scroll(
                &mut state,
                &Action::Scroll {
                    target: ScrollTarget::Result,
                    direction: ScrollDirection::Down,
                    amount: ScrollAmount::FullPage,
                },
            );

            // Should have triggered pagination
            assert_eq!(state.query.pagination.current_page, 1);
        }

        #[test]
        fn pagination_disabled_when_at_end() {
            let mut state = state_with_result_rows(100, 25);
            // visible=20, max_scroll=80, scrolled to bottom
            state.result_interaction.scroll_offset = 80;
            // Set up pagination state with no more pages
            state.query.pagination = PaginationState {
                current_page: 0,
                total_rows_estimate: Some(100),
                reached_end: true,
                schema: "public".to_string(),
                table: "users".to_string(),
            };

            // Scroll to end
            reduce_scroll(
                &mut state,
                &Action::Scroll {
                    target: ScrollTarget::Result,
                    direction: ScrollDirection::Down,
                    amount: ScrollAmount::ToEnd,
                },
            );

            // Should NOT have triggered pagination
            assert_eq!(state.query.pagination.current_page, 0);
        }

        #[test]
        fn pagination_disabled_when_not_at_bottom() {
            let mut state = state_with_result_rows(100, 25);
            // visible=20, max_scroll=80, NOT at bottom
            state.result_interaction.scroll_offset = 50;
            // Set up pagination state with more pages available
            state.query.pagination = PaginationState {
                current_page: 0,
                total_rows_estimate: Some(1500),
                reached_end: false,
                schema: "public".to_string(),
                table: "users".to_string(),
            };

            // Scroll to end
            reduce_scroll(
                &mut state,
                &Action::Scroll {
                    target: ScrollTarget::Result,
                    direction: ScrollDirection::Down,
                    amount: ScrollAmount::ToEnd,
                },
            );

            // Should NOT have triggered pagination (not at bottom)
            assert_eq!(state.query.pagination.current_page, 0);
        }

        #[test]
        fn pagination_disabled_when_adhoc_result() {
            let mut state = state_with_result_rows(100, 25);
            // visible=20, max_scroll=80, scrolled to bottom
            state.result_interaction.scroll_offset = 80;
            // Set up adhoc result (not preview)
            state.query.clear_current_result();
            let adhoc_result = QueryResult::success(
                "SELECT 1".to_string(),
                vec!["id".to_string()],
                vec![vec!["1".to_string()]],
                10,
                QuerySource::Adhoc,
            );
            state.query.set_current_result(Arc::new(adhoc_result));

            // Scroll to end
            reduce_scroll(
                &mut state,
                &Action::Scroll {
                    target: ScrollTarget::Result,
                    direction: ScrollDirection::Down,
                    amount: ScrollAmount::ToEnd,
                },
            );

            // Should NOT have triggered pagination (adhoc can't paginate)
            assert_eq!(state.query.pagination.current_page, 0);
        }

        #[test]
        fn pagination_disabled_when_no_rows() {
            let mut state = AppState::new("test".to_string());
            state.ui.result_pane_height = 25;
            // No rows
            state
                .query
                .set_current_result(Arc::new(QueryResult::success(
                    String::new(),
                    vec!["id".to_string()],
                    vec![],
                    1,
                    QuerySource::Preview,
                )));
            state.query.pagination = PaginationState {
                current_page: 0,
                total_rows_estimate: Some(100),
                reached_end: false,
                schema: "public".to_string(),
                table: "users".to_string(),
            };

            // Scroll to end
            reduce_scroll(
                &mut state,
                &Action::Scroll {
                    target: ScrollTarget::Result,
                    direction: ScrollDirection::Down,
                    amount: ScrollAmount::ToEnd,
                },
            );

            // Should NOT have triggered pagination (no rows)
            assert_eq!(state.query.pagination.current_page, 0);
        }
    }
}

