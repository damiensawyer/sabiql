use std::time::Instant;

use crate::cmd::effect::Effect;
use crate::model::app_state::AppState;
use crate::model::shared::input_mode::InputMode;
use crate::update::action::{Action, ModalKind};
use crate::update::connection::helpers::reset_active_connection_state;
use crate::update::dispatch_result::DispatchResult;
use crate::update::query_context::termination_effects;

fn selected_profile_index(state: &AppState) -> Option<usize> {
    use crate::model::connection::list::ConnectionListItem;
    match state
        .connection_list_items()
        .get(state.ui.connection_list_selected())
    {
        Some(ConnectionListItem::Profile(i)) => Some(*i),
        _ => None,
    }
}

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

        // ===== Connection Deletion (immediate, no confirm; undo with `u`) =====
        Action::RequestDeleteSelectedConnection => {
            let Some(profile_idx) = selected_profile_index(state) else {
                return DispatchResult::handled();
            };
            let Some(profile) = state.connections().get(profile_idx).cloned() else {
                return DispatchResult::handled();
            };
            if state.session.active_connection_id() == Some(&profile.id) {
                state
                    .messages
                    .set_error("Cannot delete the active connection".to_string());
                return DispatchResult::handled();
            }
            let id = profile.id.clone();
            // Push to undo buffer before deleting
            state.push_connection_delete_undo(profile);
            DispatchResult::handled_with(vec![Effect::DeleteConnection { id }])
        }
        Action::DeleteConnection(id) => {
            if state
                .session
                .pending_mysql_connection_probe()
                .is_some_and(|pending| pending.id == *id)
            {
                state.session.clear_mysql_connection_probe();
            }
            DispatchResult::handled_with(vec![Effect::DeleteConnection { id: id.clone() }])
        }
        Action::ConnectionDeleted(id) => {
            let was_active = state.session.active_connection_id() == Some(id);
            if state
                .session
                .pending_mysql_connection_probe()
                .is_some_and(|pending| pending.id == *id)
            {
                state.session.clear_mysql_connection_probe();
            }
            if was_active {
                reset_active_connection_state(state);
            }

            let id_clone = id.clone();
            state.retain_connections(move |c| c.id != id_clone);
            state.connection_caches.remove(id);

            let list_len = state.connection_list_items().len();
            if state.ui.connection_list_selected() >= list_len && list_len > 0 {
                state.ui.set_connection_list_selection(Some(list_len - 1));
            }

            if state.connections().is_empty() && state.service_entries().is_empty() {
                state.connection_setup.reset();
                state.connection_setup.set_first_run(false);
                state.modal.set_mode(InputMode::ConnectionSetup);
            }

            state
                .messages
                .set_success_at("Connection deleted (u to undo)".to_string(), now);
            DispatchResult::handled_with(if was_active {
                termination_effects(&state.query, vec![])
            } else {
                vec![]
            })
        }
        Action::ConnectionDeleteFailed(error) | Action::DuplicateConnectionFailed(error) => {
            state.messages.set_error(error.clone());
            DispatchResult::handled()
        }

        // ===== Connection Duplication =====
        Action::RequestDuplicateSelectedConnection => {
            let Some(profile_idx) = selected_profile_index(state) else {
                return DispatchResult::handled();
            };
            match state.connections().get(profile_idx) {
                Some(connection) => {
                    DispatchResult::handled_with(vec![Effect::DuplicateConnection {
                        id: connection.id.clone(),
                    }])
                }
                None => DispatchResult::handled(),
            }
        }
        Action::DuplicateConnection(id) => {
            DispatchResult::handled_with(vec![Effect::DuplicateConnection { id: id.clone() }])
        }
        Action::ConnectionDuplicated(_new_id) => {
            state
                .messages
                .set_success_at("Connection duplicated".to_string(), now);
            DispatchResult::handled_with(vec![Effect::LoadConnections])
        }

        // ===== Connection Undo =====
        Action::RequestUndoConnectionDelete => match state.pop_connection_delete_undo() {
            Some(profile) => DispatchResult::handled_with(vec![Effect::UndoConnectionDelete {
                profile: Box::new(profile),
            }]),
            None => DispatchResult::handled(),
        },
        Action::ConnectionUndo(_id) => {
            state
                .messages
                .set_success_at("Connection restored".to_string(), now);
            DispatchResult::handled_with(vec![Effect::LoadConnections])
        }

        // ===== Connection Edit =====
        Action::RequestEditSelectedConnection => {
            let Some(profile_idx) = selected_profile_index(state) else {
                return DispatchResult::handled();
            };
            match state.connections().get(profile_idx) {
                Some(connection) => {
                    DispatchResult::handled_with(vec![Effect::LoadConnectionForEdit {
                        id: connection.id.clone(),
                    }])
                }
                None => DispatchResult::handled(),
            }
        }

        _ => DispatchResult::pass(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::connection::{ConnectionId, ConnectionProfile, DatabaseType, SslMode};
    use crate::model::connection::list::build_connection_list;
    use crate::model::shared::ui_state::ResultNavMode;

    fn create_profile(name: &str) -> ConnectionProfile {
        ConnectionProfile::new_postgres(
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

    fn create_sqlite_profile(name: &str) -> ConnectionProfile {
        ConnectionProfile::new_sqlite(name.to_string(), format!("/tmp/{name}.db")).unwrap()
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

            assert_eq!(state.ui.connection_list_selected(), 0);
        }
    }

    mod request_delete_selected_connection {
        use super::*;

        #[test]
        fn pushes_profile_to_undo_buffer_before_deleting() {
            let mut state = AppState::new("test".to_string());
            let profile = create_profile("Production");
            state.set_connections(vec![profile]);
            state.ui.set_connection_list_selected_raw(0);
            state.modal.set_mode(InputMode::ConnectionSelector);

            let effects = reduce_connection_selector(
                &mut state,
                &Action::RequestDeleteSelectedConnection,
                Instant::now(),
            )
            .into_effects()
            .expect("reducer should handle action");

            // Should stay in selector mode (no confirm dialog)
            assert_eq!(state.input_mode(), InputMode::ConnectionSelector);
            // Should have pushed to undo buffer
            assert!(state.has_connection_delete_undo());
            // Should queue delete effect
            assert!(
                effects
                    .iter()
                    .any(|e| matches!(e, Effect::DeleteConnection { .. }))
            );
        }

        #[test]
        fn active_connection_cannot_be_deleted() {
            let mut state = AppState::new("test".to_string());
            let profile = create_profile("Production");
            let profile_id = profile.id.clone();
            state.set_connections(vec![profile]);
            state.ui.set_connection_list_selected_raw(0);
            state.session.activate_connection_with_dsn(
                &profile_id,
                "Production",
                DatabaseType::PostgreSQL,
                "postgres://localhost/db",
            );
            state.modal.set_mode(InputMode::ConnectionSelector);

            reduce_connection_selector(
                &mut state,
                &Action::RequestDeleteSelectedConnection,
                Instant::now(),
            );

            assert_eq!(state.input_mode(), InputMode::ConnectionSelector);
            assert!(!state.has_connection_delete_undo());
            assert!(state.messages.last_error().is_some());
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
            // Add a service so the list has items, and a LastConnection entry at index 0
            let profile = create_profile("Production");
            let profile_id = profile.id.clone();
            state.set_connections(vec![profile]);
            state.set_last_connection_id(Some(profile_id));
            state.ui.set_connection_list_selected_raw(0);
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
        use crate::domain::SqliteDiagnosticsSnapshot;
        use crate::model::connection::state::ConnectionState;
        use crate::model::er_state::ErStatus;
        use crate::model::shared::inspector_tab::InspectorTab;
        use crate::model::sql_editor::modal::SqlModalTab;
        use crate::ports::outbound::DbOperationError;
        use crate::services::AppServices;
        use crate::test_support::connection::{
            assert_explain_state_cleared, assert_sqlite_diagnostics_cleared,
        };
        use crate::update::action::ConnectionTarget;
        use crate::update::connection::lifecycle::reduce_connection_lifecycle;

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
        fn deleting_pending_probe_target_invalidates_delayed_completion() {
            let mut state = AppState::new("test".to_string());
            let current_id = ConnectionId::from_string("postgres-a");
            let deleted = create_profile("MySQL");
            let deleted_id = deleted.id.clone();
            state.set_connections(vec![deleted, create_profile("Other")]);
            state.session.activate_connection_with_dsn(
                &current_id,
                "postgres-a",
                DatabaseType::PostgreSQL,
                "postgres://localhost/a",
            );
            let target = ConnectionTarget {
                id: deleted_id.clone(),
                dsn: "mysql://user@localhost:3306/b?ssl-mode=PREFERRED".to_string(),
                name: "MySQL".to_string(),
                database_type: DatabaseType::MySQL,
                database: Some("b".to_string()),
            };
            let probe_run_id = state.session.begin_mysql_connection_probe(
                &target.id,
                &target.name,
                &target.dsn,
                target.database.as_deref(),
            );

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(deleted_id),
                Instant::now(),
            );
            reduce_connection_lifecycle(
                &mut state,
                &Action::MySqlConnectionProbeCompleted {
                    target,
                    run_id: probe_run_id,
                    lower_case_table_names: 0,
                },
                Instant::now(),
                &AppServices::stub(),
            );

            assert_eq!(state.session.active_connection_id(), Some(&current_id));
            assert!(state.session.pending_mysql_connection_probe().is_none());
        }

        #[test]
        fn delete_start_invalidates_probe_failure_before_delete_completion() {
            let mut state = AppState::new("test".to_string());
            let current_id = ConnectionId::from_string("postgres-a");
            let deleted = create_profile("MySQL");
            let deleted_id = deleted.id.clone();
            state.set_connections(vec![deleted, create_profile("Other")]);
            state.session.activate_connection_with_dsn(
                &current_id,
                "postgres-a",
                DatabaseType::PostgreSQL,
                "postgres://localhost/a",
            );
            let target = ConnectionTarget {
                id: deleted_id.clone(),
                dsn: "mysql://user@localhost:3306/b?ssl-mode=PREFERRED".to_string(),
                name: "MySQL".to_string(),
                database_type: DatabaseType::MySQL,
                database: Some("b".to_string()),
            };
            let probe_run_id = state.session.begin_mysql_connection_probe(
                &target.id,
                &target.name,
                &target.dsn,
                target.database.as_deref(),
            );

            reduce_connection_selector(
                &mut state,
                &Action::DeleteConnection(deleted_id.clone()),
                Instant::now(),
            );
            reduce_connection_lifecycle(
                &mut state,
                &Action::MySqlConnectionProbeFailed {
                    target,
                    run_id: probe_run_id,
                    error: DbOperationError::ConnectionFailed("refused".to_string()),
                },
                Instant::now(),
                &AppServices::stub(),
            );
            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(deleted_id),
                Instant::now(),
            );

            assert_eq!(state.modal.active_mode(), InputMode::Normal);
            assert!(state.connection_error.error_info().is_none());
            assert_eq!(state.session.active_connection_id(), Some(&current_id));
        }

        #[test]
        fn clears_active_state_when_active_deleted() {
            let mut state = AppState::new("test".to_string());
            let profile = create_profile("Production");
            let profile_id = profile.id.clone();
            state.set_connections(vec![profile]);
            state.session.activate_connection_with_dsn(
                &profile_id,
                "Production",
                DatabaseType::PostgreSQL,
                "postgres://localhost/db",
            );
            state
                .session
                .set_connection_state(ConnectionState::Connected);

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(profile_id),
                Instant::now(),
            );

            assert!(state.session.active_connection_id().is_none());
            assert!(state.session.dsn().is_none());
            assert!(state.session.connection_state().is_not_connected());
        }

        #[test]
        fn resets_postgres_state_when_active_deleted() {
            let mut state = AppState::new("test".to_string());
            let profile = create_profile("Production");
            let profile_id = profile.id.clone();
            state.set_connections(vec![profile]);
            state.session.activate_connection_with_dsn(
                &profile_id,
                "Production",
                DatabaseType::PostgreSQL,
                "postgres://localhost/db",
            );
            state
                .session
                .set_connection_state(ConnectionState::Connected);

            // Set state that was previously not reset by ConnectionDeleted.
            state.query.pagination.set_current_page(3);
            state.result_interaction.activate_cell(5, 0);
            state.result_interaction.set_scroll_offset(10);
            state.result_interaction.set_horizontal_offset(20);
            state.result_interaction.stage_row(0);
            state.ui.set_inspector_tab(InspectorTab::Rls);
            state.ui.set_inspector_scroll_offset(17);
            state.ui.set_inspector_horizontal_offset(23);
            state.sql_modal.set_active_tab(SqlModalTab::Compare);
            state.explain.set_plan(
                "Seq Scan  (cost=0.00..100.00 rows=10 width=32)".to_string(),
                DatabaseType::PostgreSQL,
                false,
                0,
                "SELECT * FROM users",
            );
            state.explain.set_plan(
                "Index Scan  (cost=0.00..5.00 rows=1 width=32)".to_string(),
                DatabaseType::PostgreSQL,
                false,
                0,
                "SELECT * FROM users WHERE id = 1",
            );
            state.explain.set_error("stale error".to_string());
            state.ui.set_pending_er_picker(true);
            let _ = state.er_preparation.start_waiting_run();
            state
                .table_prefetch
                .queue_table_prefetch("public.users".to_string());

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(profile_id),
                Instant::now(),
            );

            assert_eq!(state.query.pagination.current_page(), 0);
            assert_eq!(
                state.result_interaction.selection().mode(),
                ResultNavMode::Scroll
            );
            assert_eq!(state.result_interaction.scroll_offset(), 0);
            assert_eq!(state.result_interaction.horizontal_offset(), 0);
            assert!(state.result_interaction.staged_delete_rows().is_empty());
            assert!(state.result_interaction.pending_write_preview().is_none());
            assert_eq!(state.ui.inspector_tab(), InspectorTab::Info);
            assert_eq!(state.ui.inspector_scroll_offset(), 0);
            assert_eq!(state.ui.inspector_horizontal_offset(), 0);
            assert_eq!(state.sql_modal.active_tab(), SqlModalTab::Sql);
            assert_explain_state_cleared(&state);
            assert_sqlite_diagnostics_cleared(&state);
            assert!(!state.ui.pending_er_picker());
            assert_eq!(state.er_preparation.status(), ErStatus::Idle);
            assert!(!state.table_prefetch.has_pending_prefetch());
        }

        #[test]
        fn resets_sqlite_state_when_active_deleted() {
            let mut state = AppState::new("test".to_string());
            let profile = create_sqlite_profile("Production");
            let profile_id = profile.id.clone();
            state.set_connections(vec![profile]);
            state.session.activate_connection_with_dsn(
                &profile_id,
                "Production",
                DatabaseType::SQLite,
                "sqlite:///tmp/Production.db",
            );
            state
                .session
                .set_connection_state(ConnectionState::Connected);

            state.ui.set_inspector_tab(InspectorTab::Ddl);
            state.ui.set_inspector_scroll_offset(17);
            state.ui.set_inspector_horizontal_offset(23);
            state.sql_modal.set_active_tab(SqlModalTab::Plan);
            state.explain.set_plan(
                "SCAN users".to_string(),
                DatabaseType::SQLite,
                false,
                0,
                "SELECT * FROM users",
            );
            state.explain.set_error("stale error".to_string());
            let diagnostics_run_id = state.sqlite_diagnostics.begin_core_fetch();
            state
                .sqlite_diagnostics
                .set_core_loaded(diagnostics_run_id, SqliteDiagnosticsSnapshot::default());
            let _ = state.sqlite_diagnostics.begin_quick_check();

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(profile_id),
                Instant::now(),
            );

            assert_eq!(state.ui.inspector_tab(), InspectorTab::Info);
            assert_eq!(state.ui.inspector_scroll_offset(), 0);
            assert_eq!(state.ui.inspector_horizontal_offset(), 0);
            assert_eq!(state.sql_modal.active_tab(), SqlModalTab::Sql);
            assert_explain_state_cleared(&state);
            assert_sqlite_diagnostics_cleared(&state);
        }

        #[test]
        fn adjusts_selection_when_last_item_deleted() {
            let mut state = AppState::new("test".to_string());
            let profile1 = create_profile("First");
            let profile2 = create_profile("Second");
            let id_to_delete = profile2.id.clone();
            state.set_connections(vec![profile1, profile2]);
            state.ui.set_connection_list_selected_raw(1);

            reduce_connection_selector(
                &mut state,
                &Action::ConnectionDeleted(id_to_delete),
                Instant::now(),
            );

            assert_eq!(state.ui.connection_list_selected(), 0);
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

            assert_eq!(
                state.connection_list_items(),
                build_connection_list(1, 0, false)
            );
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
            assert_eq!(
                state.connection_list_items(),
                build_connection_list(0, 1, false)
            );
        }
    }
}
