use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use crate::app::model::app_state::AppState;
use crate::app::model::connection::list::{
    ConnectionListItem, has_last_connection,
};
use crate::app::update::input::keybindings::connection_selector;
use crate::domain::connection::ConnectionId;
use crate::primitives::atoms::scroll_indicator::{
    VerticalScrollParams, render_vertical_scroll_indicator_bar,
};
use crate::primitives::molecules::{FooterHintBar, render_modal};
use crate::theme::ThemePalette;

const PREFIX_DISPLAY_WIDTH: usize = 2;
const SERVICE_LABEL_COL_PERCENT: usize = 40;
const DIVIDER_HEIGHT: u16 = 1;

pub struct ConnectionSelector;

impl ConnectionSelector {
    pub fn render(frame: &mut Frame, state: &AppState, theme: &ThemePalette) -> u16 {
        let has_last = has_last_connection(state.connection_list_items());
        let is_service_selected = crate::app::model::connection::list::is_service_selected(
            state.connection_list_items(),
            state.ui.connection_list_selected,
        );
        let hint = Self::build_hints(is_service_selected, has_last);
        let (_outer, inner) = render_modal(
            frame,
            Constraint::Percentage(60),
            Constraint::Percentage(60),
            " Select Connection ",
            FooterHintBar::new(hint),
            theme,
        );

        if has_last {
            Self::render_two_panel_list(frame, inner, state, theme)
        } else {
            render_connection_list(frame, inner, state, theme, false)
        }
    }

    fn render_two_panel_list(
        frame: &mut Frame,
        area: Rect,
        state: &AppState,
        theme: &ThemePalette,
    ) -> u16 {
        let all_items = state.connection_list_items();
        let last_conn_item = match all_items.first() {
            Some(ConnectionListItem::LastConnection) => all_items[0].clone(),
            _ => return area.height,
        };

        // Determine the last connection profile for display
        let last_conn_display = if let ConnectionListItem::LastConnection = last_conn_item {
            state
                .last_connection_profile()
                .map(|c| c.display_name().to_string())
        } else {
            None
        };

        let total_height = area.height;
        // Reserve 1 line for the last connection, 1 for divider
        let bottom_available = total_height.saturating_sub(DIVIDER_HEIGHT + 2);
        let bottom_height = if bottom_available >= 2 { bottom_available } else { total_height };
        let last_height = if total_height > bottom_height + DIVIDER_HEIGHT + 1 {
            total_height - bottom_height - DIVIDER_HEIGHT
        } else {
            1
        };

        let chunks = Layout::vertical([
            Constraint::Length(last_height),
            Constraint::Length(DIVIDER_HEIGHT),
            Constraint::Length(bottom_height),
        ])
        .split(area);

        // Top panel: last connection — real List widget so the cursor moves here on Tab
        if let Some(name) = last_conn_display {
            let is_selected = state.ui.connection_list_selected == 0;
            let style = if is_selected {
                Style::default()
                    .fg(theme.semantic.text.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.semantic.text.secondary)
            };
            let item = ListItem::new(format!("  Open last: {name}  ")).style(style);

            let list = List::new(vec![item])
                .highlight_style(
                    Style::default()
                        .fg(theme.semantic.text.accent)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(">");

            let mut top_list_state = ListState::default()
                .with_selected(if is_selected { Some(0) } else { None });
            frame.render_stateful_widget(list, chunks[0], &mut top_list_state);
        }

        // Divider
        let divider_text = "────────────────────────────────────";
        let divider_para = Paragraph::new(divider_text)
            .style(Style::default().fg(theme.semantic.text.muted));
        frame.render_widget(divider_para, chunks[1]);

        // Bottom panel: regular connections
        render_connection_list(frame, chunks[2], state, theme, true);
        area.height
    }

    fn build_hints(
        is_service_selected: bool,
        has_last: bool,
    ) -> Vec<(&'static str, &'static str)> {
        use connection_selector as cs;

        let mut hints = vec![cs::CONFIRM.as_hint(), cs::NEW.as_hint()];
        if !is_service_selected {
            hints.push(cs::EDIT.as_hint());
            hints.push(cs::DELETE.as_hint());
            hints.push(cs::DUPLICATE.as_hint());
        }
        if has_last {
            hints.push(("Tab/⇧Tab", "Toggle panels"));
        }
        hints.push(cs::CLOSE.as_hint());

        hints
    }
}

fn active_prefix(is_active: bool) -> &'static str {
    if is_active { "● " } else { "  " }
}

fn render_profile_item(
    id: &ConnectionId,
    display_name: &str,
    active_id: Option<&ConnectionId>,
    theme: &ThemePalette,
) -> ListItem<'static> {
    let is_active = active_id == Some(id);
    let prefix = active_prefix(is_active);
    let text = format!("{prefix}{display_name}");
    let style = if is_active {
        Style::default().fg(theme.component.navigation.active_indicator)
    } else {
        Style::default().fg(theme.semantic.text.secondary)
    };
    ListItem::new(text).style(style)
}

fn render_service_item(
    display_name: &str,
    service_id: ConnectionId,
    active_id: Option<&ConnectionId>,
    content_width: usize,
    source_label: &str,
    theme: &ThemePalette,
) -> ListItem<'static> {
    let is_active = active_id == Some(&service_id);
    let prefix = active_prefix(is_active);
    let min_gap = 2;
    let max_name_len =
        content_width.saturating_sub(PREFIX_DISPLAY_WIDTH + min_gap + source_label.len());

    let name = if display_name.chars().count() > max_name_len {
        let truncated: String = display_name
            .chars()
            .take(max_name_len.saturating_sub(1))
            .collect();
        format!("{truncated}…")
    } else {
        display_name.to_owned()
    };

    let label_col = content_width * SERVICE_LABEL_COL_PERCENT / 100;
    let name_display_width = PREFIX_DISPLAY_WIDTH + name.chars().count();
    let gap = label_col.saturating_sub(name_display_width).max(min_gap);
    let name_part = format!("{prefix}{name}");

    let name_style = if is_active {
        Style::default().fg(theme.component.navigation.active_indicator)
    } else {
        Style::default().fg(theme.semantic.text.secondary)
    };
    let line = Line::from(vec![
        Span::styled(name_part, name_style),
        Span::raw(" ".repeat(gap)),
        Span::styled(
            source_label.to_owned(),
            Style::default().fg(theme.semantic.text.muted),
        ),
    ]);
    ListItem::new(line)
}

pub fn render_connection_list(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    theme: &ThemePalette,
    has_top_panel: bool,
) -> u16 {
    let active_id = state.session.active_connection_id.as_ref();

    // highlight_symbol "> " takes 2 columns
    let content_width = area.width.saturating_sub(2) as usize;
    let source_label = "from pg_service.conf";

    // Bottom panel excludes LastConnection (already shown in top panel)
    let bottom_items =
        crate::app::model::connection::list::bottom_panel_items(state.connection_list_items());
    let items: Vec<ListItem> = if bottom_items.is_empty() {
        vec![ListItem::new(" No connections")]
    } else {
        bottom_items
            .iter()
            .map(|item| match item {
                ConnectionListItem::Profile(i) => {
                    let conn = &state.connections()[*i];
                    render_profile_item(&conn.id, conn.display_name(), active_id, theme)
                }
                ConnectionListItem::Service(i) => {
                    let entry = &state.service_entries()[*i];
                    render_service_item(
                        entry.display_name(),
                        entry.connection_id(),
                        active_id,
                        content_width,
                        source_label,
                        theme,
                    )
                }
                ConnectionListItem::LastConnection => {
                    // Last connection displayed in bottom panel too
                    ListItem::new("")
                }
            })
            .collect()
    };

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(theme.semantic.text.accent)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    // When a top panel is shown, the bottom panel's item indices are offset
    // by one (the LastConnection item at index 0 is excluded). Adjust the
    // highlight index accordingly so the selection marker appears on the
    // correct visual row.
    let adjusted_selected = if has_top_panel && state.ui.connection_list_selected > 0 {
        state.ui.connection_list_selected - 1
    } else {
        state.ui.connection_list_selected
    };

    let mut list_state = ListState::default()
        .with_selected(Some(adjusted_selected))
        .with_offset(state.ui.connection_list_scroll_offset);
    frame.render_stateful_widget(list, area, &mut list_state);

    if !bottom_items.is_empty() {
        let total_items = bottom_items.len();
        let viewport_size = area.height as usize;

        if total_items > viewport_size {
            let scroll_offset = state.ui.connection_list_scroll_offset;

            render_vertical_scroll_indicator_bar(
                frame,
                area,
                VerticalScrollParams {
                    position: scroll_offset,
                    viewport_size,
                    total_items,
                    has_horizontal_scrollbar: false,
                },
                theme,
            );
        }
    }

    area.height
}
