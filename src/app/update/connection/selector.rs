use std::time::Instant;

use crate::cmd::effect::Effect;
use crate::model::app_state::AppState;
use crate::model::shared::input_mode::InputMode;
use crate::update::action::{Action, ModalKind};
use crate::update::dispatch_result::DispatchResult;

pub(super) fn reduce_connection_selector(
    state: &mut AppState,
    action: &Action,
    now: Instant,
) -> DispatchResult {
    match action {
        Action::OpenModal(ModalKind::ConnectionSelector) => {
            state.modal.set_mode(InputMode::ConnectionSelector);
            state.ui.set_connection_list_selection(Some(0));
            DispatchResult::handled_with(vec![Effect::LoadConnections])
        }

        // ===== Connection Deletion (immediate, no confirm) =====
        Action::RequestDeleteSelectedConnection => {
            use crate::model::connection::list::ConnectionListItem;
            let selected_idx = state.ui.connection_list_selected;
            let profile_idx = match state.connection_list_items().get(selected_idx) {
                Some(ConnectionListItem::Profile(i)) => *i,
                _ => return DispatchResult::handled(),
            };
            if let Some(connection) = state.connections().get(profile_idx) {
                let id = connection.id.clone();
                let is_active = state.session.active_connection_id.as_ref() == Some(&id);

                if is_active {
                    // Can't delete active connection - show error
                    state
                        .messages
                        .set_error_at("Cannot delete the active connection".to_string(), now);
                    return DispatchResult::handled();
                }

                // Push to undo buffer before deleting
                if let Some(profile) = state.connections().get(profile_idx).cloned() {
                    state.push_connection_delete_undo(profile);
                }

                DispatchResult::handled_with(vec![Effect::DeleteConnection { id }])
            } else {
                DispatchResult::handled()
            }
        }
        Action::DeleteConnection(id) => {
            DispatchResult::handled_with(vec![Effect::DeleteConnection { id: id.clone() }])
        }
        Action::ConnectionDeleted(id) => {
            if state.session.active_connection_id.as_ref() == Some(id) {
                state.session.reset(&mut state.query);
                state.result_interaction.reset_view();
                state.ui.set_explorer_selection(None);
            }

            let id_clone = id.clone();
            state.retain_connections(move |c| c.id != id_clone);
            state.connection_caches.remove(id);

            let list_len = state.connection_list_items().len();
            if state.ui.connection_list_selected >= list_len && list_len > 0 {
                state.ui.set_connection_list_selection(Some(list_len - 1));
            }

            if state.connections().is_empty() && state.service_entries().is_empty() {
                state.connection_setup.reset();
                state.connection_setup.is_first_run = false;
                state.modal.set_mode(InputMode::ConnectionSetup);
            }

            state
                .messages
                .set_success_at("Connection deleted".to_string(), now);
            DispatchResult::handled()
        }

        // ===== Connection Duplication =====
        Action::RequestDuplicateSelectedConnection => {
            use crate::model::connection::list::ConnectionListItem;
            let selected_idx = state.ui.connection_list_selected;
            let profile_idx = match state.connection_list_items().get(selected_idx) {
                Some(ConnectionListItem::Profile(i)) => *i,
                _ => return DispatchResult::handled(),
            };
            if let Some(connection) = state.connections().get(profile_idx) {
                let id = connection.id.clone();
                DispatchResult::handled_with(vec![Effect::DuplicateConnection { id }])
            } else {
                DispatchResult::handled()
            }
        }
        Action::ConnectionDuplicated(_new_id) => {
            state
                .messages
                .set_success_at("Connection duplicated".to_string(), now);
            DispatchResult::handled_with(vec![Effect::LoadConnections])
        }
        Action::DuplicateConnectionFailed(e) => {
            state.messages.set_error_at(e.to_string(), now);
            DispatchResult::handled()
        }

        // ===== Connection Undo =====
        Action::RequestUndoConnectionDelete => {
            if let Some(profile) = state.pop_connection_delete_undo() {
                let _id = profile.id.clone();
                DispatchResult::handled_with(vec![Effect::UndoConnectionDelete {
                    profile: Box::new(profile),
                }])
            } else {
                DispatchResult::handled()
            }
        }
        Action::ConnectionUndo(_id) => {
            state
                .messages
                .set_success_at("Connection restored".to_string(), now);
            DispatchResult::handled_with(vec![Effect::LoadConnections])
        }
        Action::ConnectionDeleteFailed(e) => {
            state.messages.set_error_at(e.to_string(), now);
            DispatchResult::handled()
        }

        // ===== Connection Edit =====
        Action::RequestEditSelectedConnection => {
            use crate::model::connection::list::ConnectionListItem;
            let selected_idx = state.ui.connection_list_selected;
            let profile_idx = match state.connection_list_items().get(selected_idx) {
                Some(ConnectionListItem::Profile(i)) => *i,
                _ => return DispatchResult::handled(),
            };
            if let Some(connection) = state.connections().get(profile_idx) {
                let id = connection.id.clone();
                DispatchResult::handled_with(vec![Effect::LoadConnectionForEdit { id }])
            } else {
                DispatchResult::handled()
            }
        }

        _ => DispatchResult::pass(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::connection::{ConnectionProfile, SslMode};
    use crate::model::connection::list::build_connection_list;

    fn create_profile(name: &str) -> ConnectionProfile {
        ConnectionProfile::new(
            name.to_string(),
            "localhost".to_string(),
            5432,
            "db".to_string(),
            "user".to_string(),
            "pass".to_string(),
            SslMode::default(),
        )
        .unwrap()
    }

    mod open_connection_selector {
        use super::*;

        #[test]
        fn sets_mode_and_loads_connections() {
            let mut state = AppState::new("test".to_string());

            let effects = reduce_connection_selector(
                &mut state,
                &Action::OpenModal(ModalKind::ConnectionSelector),
                Instant::now(),
            );

            assert_eq!(state.input_mode(), InputMode::ConnectionSelector);
            let effects = effects
                .into_effects()
                .expect("reducer should handle action");
            assert!(effects.iter().any(|e| matches!(e, Effect::LoadConnections)));
        }

        #[test]
        fn resets_selection_to_zero() {
            let mut state = AppState::new("test".to_string());
            state.ui.set_connection_list_selection(Some(3));

            reduce_connection_selector(
                &mut state,
                &Action::OpenModal(ModalKind::ConnectionSelector),
                Instant::now(),
            );

            assert_eq!(state.ui.connection_list_selected, 0);
        }
    }

    mod request_delete_selected_connection {
        use super::*;

        #[test]
        fn pushes_profile_to_undo_buffer_before_deleting() {
            let mut state = AppState::new("test".to_string());
            let profile = create_profile("Production");
            let profile_id = profile.id.clone();
            state.set_connections(vec![profile.clone()]);
            state.ui.connection_list_selected = 0;
            state.modal.set_mode(InputMode::ConnectionSelector);

            reduce_connection_selector(
                &mut state,
                &Action::RequestDeleteSelectedConnection,
                Instant::now(),
            );

            // Should stay in selector mode (no confirm dialog)
            assert_eq!(state.input_mode(), InputMode::ConnectionSelector);
            // Should have pushed to undo buffer
            assert!(state.has_connection_delete_undo());
            // Should queue delete effect
            let effects = state
                .messages
                .last_error
                .clone()
                .or_else(|| state.messages.last_success.clone());
            // Verify delete effect is queued (not confirm dialog)
            assert_ne!(state.input_mode(), InputMode::ConfirmDialog);
        }

        #[test]
        fn active_connection_cannot_be_deleted() {
            let mut state = AppState::new("test".to_string());
            let profile = create_profile("Production");
            let profile_id = profile.id.clone();
            state.set_connections(vec![profile]);
            state.ui.connection_list_selected = 0;
            state.session.active_connection_id = Some(profile_id);
            state.modal.set_mode(InputMode::ConnectionSelector);

            reduce_connection_selector(
                &mut state,
                &Action::RequestDeleteSelectedConnection,
                Instant::now(),
            );

            assert_eq!(state.input_mode(), InputMode::ConnectionSelector);
            // Should show error message
            assert!(state.messages.last_error.is_some());
        }

        #[test]
        fn empty_list_does_nothing() {
            let mut state = AppState::new("test".to_string());
            state.set_connections(vec![]);

            reduce_connection_selector(
                &mut state,
                &Action::RequestDeleteSelectedConnection,
                Instant::now(),
            );

            assert_eq!(state.input_mode(), InputMode::Normal);
        }

        #[test]
        fn last_connection_item_cannot_be_deleted() {
            let mut state = AppState::new("test".to_string());
            state.set_connections(vec![]);
            // Add a service so the list has items
            use crate::domain::connection::ServiceEntry;
            state.set_connections_and_services(
                vec![],
                vec![ServiceEntry {
                    service_name: "mydb".to_string(),
                    host: None,
                    dbname: None,
                    port: None,
                    user: None,
                }],
            );
            // Select LastConnection (index 0)
            state.ui.connection_list_selected = 0;
            state.modal.set_mode(InputMode::ConnectionSelector);

            reduce_connection_selector(
                &mut state,
                &Action::RequestDeleteSelectedConnection,
                Instant::now(),
            );

            // Should do nothing - LastConnection is not a profile
            assert_eq!(state.input_mode(), InputMode::ConnectionSelector);
        }
    }

    mod connection_deleted {
        use super::*;
        use crate::model::connection::state::ConnectionState;

        #[test]
        fn removes_connection_from_list() {
            let mut state = AppState::new("test".to_string());
            let profile1 = create_profile("First");
            let profile2 = create_profile("Second");
            let id_to_delete = profile1.id.clone();
            state.set_connections(vec![profile1, profile2]);

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(id_to_delete),
                Instant::now(),
            );

            assert_eq!(state.connections().len(), 1);
            assert_eq!(state.connections()[0].name.as_str(), "Second");
        }

        #[test]
        fn clears_active_state_when_active_deleted() {
            let mut state = AppState::new("test".to_string());
            let profile = create_profile("Production");
            let profile_id = profile.id.clone();
            state.set_connections(vec![profile]);
            state.session.active_connection_id = Some(profile_id.clone());
            state.session.dsn = Some("postgres://localhost/db".to_string());
            state
                .session
                .set_connection_state(ConnectionState::Connected);

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(profile_id),
                Instant::now(),
            );

            assert!(state.session.active_connection_id.is_none());
            assert!(state.session.dsn.is_none());
            assert!(state.session.connection_state().is_not_connected());
        }

        #[test]
        fn resets_full_state_when_active_deleted() {
            let mut state = AppState::new("test".to_string());
            let profile = create_profile("Production");
            let profile_id = profile.id.clone();
            state.set_connections(vec![profile]);
            state.session.active_connection_id = Some(profile_id.clone());
            state.session.dsn = Some("postgres://localhost/db".to_string());
            state
                .session
                .set_connection_state(ConnectionState::Connected);

            // Set state that was previously not reset by ConnectionDeleted
            state.query.pagination.current_page = 3;
            state.result_interaction.activate_cell(5, 0);
            state.result_interaction.scroll_offset = 10;
            state.result_interaction.horizontal_offset = 20;
            state.result_interaction.stage_row(0);

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(profile_id),
                Instant::now(),
            );

            assert_eq!(state.query.pagination.current_page, 0);
            assert_eq!(
                state.result_interaction.selection().mode(),
                crate::model::shared::ui_state::ResultNavMode::Scroll
            );
            assert_eq!(state.result_interaction.scroll_offset, 0);
            assert_eq!(state.result_interaction.horizontal_offset, 0);
            assert!(state.result_interaction.staged_delete_rows().is_empty());
            assert!(state.result_interaction.pending_write_preview().is_none());
        }

        #[test]
        fn adjusts_selection_when_last_item_deleted() {
            let mut state = AppState::new("test".to_string());
            let profile1 = create_profile("First");
            let profile2 = create_profile("Second");
            let id_to_delete = profile2.id.clone();
            state.set_connections(vec![profile1, profile2]);
            state.ui.connection_list_selected = 1;

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(id_to_delete),
                Instant::now(),
            );

            assert_eq!(state.ui.connection_list_selected, 0);
        }

        #[test]
        fn transitions_to_setup_when_list_empty() {
            let mut state = AppState::new("test".to_string());
            let profile = create_profile("Only");
            let profile_id = profile.id.clone();
            state.set_connections(vec![profile]);

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(profile_id),
                Instant::now(),
            );

            assert!(state.connections().is_empty());
            assert_eq!(state.input_mode(), InputMode::ConnectionSetup);
        }

        #[test]
        fn rebuilds_connection_list_items_after_delete() {
            let mut state = AppState::new("test".to_string());
            let profile1 = create_profile("First");
            let profile2 = create_profile("Second");
            let id_to_delete = profile1.id.clone();
            state.set_connections(vec![profile1, profile2]);

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(id_to_delete),
                Instant::now(),
            );

            assert_eq!(state.connection_list_items(), build_connection_list(1, 0, false));
        }

        #[test]
        fn stays_in_selector_when_services_remain_after_last_profile_deleted() {
            use crate::domain::connection::ServiceEntry;

            let mut state = AppState::new("test".to_string());
            let profile = create_profile("Only");
            let profile_id = profile.id.clone();
            state.set_connections_and_services(
                vec![profile],
                vec![ServiceEntry {
                    service_name: "mydb".to_string(),
                    host: None,
                    dbname: None,
                    port: None,
                    user: None,
                }],
            );
            state.modal.set_mode(InputMode::Normal);

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(profile_id),
                Instant::now(),
            );

            assert!(state.connections().is_empty());
            assert_ne!(state.input_mode(), InputMode::ConnectionSetup);
            assert_eq!(state.connection_list_items(), build_connection_list(0, 1, false));
        }
    }
}
