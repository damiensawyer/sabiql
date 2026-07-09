use std::time::Instant;

use crate::cmd::effect::Effect;
use crate::model::app_state::AppState;
use crate::model::shared::input_mode::InputMode;
use crate::services::AppServices;
use crate::update::action::{Action, ConnectionTarget};
use crate::update::dispatch_result::DispatchResult;

use super::helpers::{restore_cache, save_current_cache};

pub(super) fn reduce_connection_lifecycle(
    state: &mut AppState,
    action: &Action,
    _now: Instant,
    services: &AppServices,
) -> DispatchResult {
    match action {
        Action::TryConnect => {
            if state.session.connection_state().is_not_connected()
                && state.modal.active_mode() == InputMode::Normal
            {
                if let Some(dsn) = state.session.dsn.clone() {
                    let run_id = state.session.begin_connecting(&dsn);
                    DispatchResult::handled_with(vec![Effect::FetchMetadata { dsn, run_id }])
                } else {
                    DispatchResult::handled()
                }
            } else {
                DispatchResult::handled()
            }
        }

        Action::SwitchToLastConnection => {
            // Save the current active connection as the next toggle target,
            // so pressing C again will toggle back to this one.
            if let Some(current_id) = state.session.active_connection_id.clone() {
                state.set_last_connection_id(Some(current_id));
            }
            // Build the SwitchConnection action from the last stored connection
            let last_id = state.last_connection_id().cloned();
            if let Some(ref lid) = last_id {
                let profile = state
                    .connections()
                    .iter()
                    .find(|c| &c.id == lid);
                if let Some(profile) = profile {
                    let dsn = services.dsn_builder.build_dsn(profile);
                    let target = ConnectionTarget {
                        id: profile.id.clone(),
                        dsn,
                        name: profile.display_name().to_string(),
                    };
                    return DispatchResult::handled_with(vec![Effect::DispatchActions(vec![
                        Action::SwitchConnection(target),
                    ])]);
                }
            }
            DispatchResult::handled()
        }

        Action::SwitchConnection(ConnectionTarget { id, dsn, name }) => {
            if let Some(current_id) = state.session.active_connection_id.clone() {
                let cache = save_current_cache(state);
                state.connection_caches.save(&current_id, cache);
            }

            // Try to restore from cache
            if let Some(cached) = state.connection_caches.get(id).cloned() {
                restore_cache(state, &cached, services);
                state.session.active_connection_id = Some(id.clone());
                state.session.dsn = Some(dsn.clone());
                state.session.active_connection_name = Some(name.clone());
                state.session.read_only = false;
                DispatchResult::handled_with(vec![Effect::ClearCompletionEngineCache])
            } else {
                // No cache: reset and fetch metadata
                state.session.reset(&mut state.query);
                state.result_interaction.reset_view();
                state.ui.set_explorer_selection(None);
                state.session.active_connection_id = Some(id.clone());
                state.session.active_connection_name = Some(name.clone());
                state.session.read_only = false;
                let run_id = state.session.begin_connecting(dsn);
                DispatchResult::handled_with(vec![
                    Effect::ClearCompletionEngineCache,
                    Effect::FetchMetadata {
                        dsn: dsn.clone(),
                        run_id,
                    },
                ])
            }
        }

        _ => DispatchResult::pass(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ConnectionId;
    use crate::model::connection::cache::ConnectionCache;
    use crate::model::connection::state::ConnectionState;
    use crate::model::shared::inspector_tab::InspectorTab;

    fn create_switch_action(id: &ConnectionId, name: &str) -> Action {
        Action::SwitchConnection(ConnectionTarget {
            id: id.clone(),
            dsn: format!("postgres://localhost/{name}"),
            name: name.to_string(),
        })
    }

    #[test]
    fn saves_current_cache_before_switching() {
        let mut state = AppState::new("test".to_string());
        let current_id = ConnectionId::new();
        let new_id = ConnectionId::new();
        let services = AppServices::stub();

        state.session.active_connection_id = Some(current_id.clone());
        state.ui.explorer_selected = 5;
        state.ui.inspector_tab = InspectorTab::Indexes;

        let action = create_switch_action(&new_id, "new_db");
        reduce_connection_lifecycle(&mut state, &action, Instant::now(), &services);

        let saved = state.connection_caches.get(&current_id).unwrap();
        assert_eq!(saved.explorer_selected, 5);
        assert_eq!(saved.inspector_tab, InspectorTab::Indexes);
    }

    #[test]
    fn restores_cached_state_when_available() {
        let mut state = AppState::new("test".to_string());
        let target_id = ConnectionId::new();
        let services = AppServices::stub();

        let cached = ConnectionCache {
            explorer_selected: 42,
            inspector_tab: InspectorTab::ForeignKeys,
            ..Default::default()
        };
        state.connection_caches.save(&target_id, cached);

        let action = create_switch_action(&target_id, "cached_db");
        reduce_connection_lifecycle(&mut state, &action, Instant::now(), &services);

        assert_eq!(state.ui.explorer_selected, 42);
        assert_eq!(state.ui.inspector_tab, InspectorTab::ForeignKeys);
    }

    #[test]
    fn normalizes_cached_inspector_tab_when_capability_is_missing() {
        let mut state = AppState::new("test".to_string());
        let target_id = ConnectionId::new();
        let mut services = AppServices::stub();
        services.db_capabilities = crate::model::shared::db_capabilities::DbCapabilities::new(
            true,
            vec![InspectorTab::Info],
        );

        let cached = ConnectionCache {
            explorer_selected: 42,
            inspector_tab: InspectorTab::Ddl,
            ..Default::default()
        };
        state.connection_caches.save(&target_id, cached);

        let action = create_switch_action(&target_id, "cached_db");
        reduce_connection_lifecycle(&mut state, &action, Instant::now(), &services);

        assert_eq!(state.ui.inspector_tab, InspectorTab::Info);
    }

    #[test]
    fn fetches_metadata_when_no_cache_exists() {
        let mut state = AppState::new("test".to_string());
        let new_id = ConnectionId::new();
        let services = AppServices::stub();

        let action = create_switch_action(&new_id, "fresh_db");
        let effects = reduce_connection_lifecycle(&mut state, &action, Instant::now(), &services)
            .into_effects()
            .expect("reducer should handle action");

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::FetchMetadata { .. }))
        );
        assert_eq!(
            state.session.connection_state(),
            ConnectionState::Connecting
        );
    }

    #[test]
    fn updates_active_connection_fields() {
        let mut state = AppState::new("test".to_string());
        let new_id = ConnectionId::new();
        let services = AppServices::stub();

        let action = create_switch_action(&new_id, "target_db");
        reduce_connection_lifecycle(&mut state, &action, Instant::now(), &services);

        assert_eq!(state.session.active_connection_id, Some(new_id));
        assert_eq!(
            state.session.dsn,
            Some("postgres://localhost/target_db".to_string())
        );
        assert_eq!(
            state.session.active_connection_name,
            Some("target_db".to_string())
        );
    }

    #[test]
    fn sets_connected_state_when_cache_exists() {
        let mut state = AppState::new("test".to_string());
        let target_id = ConnectionId::new();
        let services = AppServices::stub();

        state
            .connection_caches
            .save(&target_id, ConnectionCache::default());

        let action = create_switch_action(&target_id, "cached_db");
        reduce_connection_lifecycle(&mut state, &action, Instant::now(), &services);

        assert_eq!(state.session.connection_state(), ConnectionState::Connected);
    }

    #[test]
    fn resets_result_selection_when_restoring_cache() {
        let mut state = AppState::new("test".to_string());
        let target_id = ConnectionId::new();
        let services = AppServices::stub();

        state
            .connection_caches
            .save(&target_id, ConnectionCache::default());
        state.result_interaction.activate_cell(3, 2);

        let action = create_switch_action(&target_id, "cached_db");
        reduce_connection_lifecycle(&mut state, &action, Instant::now(), &services);

        assert_eq!(
            state.result_interaction.selection().mode(),
            crate::model::shared::ui_state::ResultNavMode::Scroll
        );
    }

    #[test]
    fn resets_result_selection_when_no_cache() {
        let mut state = AppState::new("test".to_string());
        let new_id = ConnectionId::new();
        let services = AppServices::stub();

        state.result_interaction.activate_cell(5, 0);

        let action = create_switch_action(&new_id, "fresh_db");
        reduce_connection_lifecycle(&mut state, &action, Instant::now(), &services);

        assert_eq!(
            state.result_interaction.selection().mode(),
            crate::model::shared::ui_state::ResultNavMode::Scroll
        );
    }

    #[test]
    fn resets_read_only_on_switch() {
        let mut state = AppState::new("test".to_string());
        let new_id = ConnectionId::new();
        let services = AppServices::stub();
        state.session.read_only = true;

        let action = create_switch_action(&new_id, "fresh_db");
        reduce_connection_lifecycle(&mut state, &action, Instant::now(), &services);

        assert!(!state.session.read_only);
    }

    #[test]
    fn clears_completion_cache_on_switch() {
        let mut state = AppState::new("test".to_string());
        let new_id = ConnectionId::new();
        let services = AppServices::stub();

        let action = create_switch_action(&new_id, "any_db");
        let effects = reduce_connection_lifecycle(&mut state, &action, Instant::now(), &services)
            .into_effects()
            .expect("reducer should handle action");

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::ClearCompletionEngineCache))
        );
    }

    mod switch_to_last_connection {
        use super::*;
        use crate::domain::connection::{ConnectionName, ConnectionProfile, SslMode};
        use crate::update::action::Action;

        fn make_profile(name: &str) -> ConnectionProfile {
            ConnectionProfile {
                id: ConnectionId::new(),
                name: ConnectionName::new(name).unwrap(),
                host: "localhost".to_string(),
                port: 5432,
                database: "testdb".to_string(),
                username: "user".to_string(),
                password: "pass".to_string(),
                ssl_mode: SslMode::Prefer,
            }
        }

        #[test]
        fn dispatches_switch_connection_when_last_connection_exists() {
            let mut state = AppState::new("test".to_string());
            let profile = make_profile("last_conn");
            let last_id = profile.id.clone();
            state.set_connections(vec![profile]);
            state.set_last_connection_id(Some(last_id.clone()));

            let services = AppServices::stub();

            let action = Action::SwitchToLastConnection;
            let result = reduce_connection_lifecycle(
                &mut state,
                &action,
                Instant::now(),
                &services,
            );

            // Should return handled result with effects
            let effects = result.into_effects().expect("should be handled");
            assert_eq!(effects.len(), 1, "should have 1 effect, got {:?}", effects);
            // Effect should be DispatchActions containing SwitchConnection
            match &effects[0] {
                Effect::DispatchActions(actions) => {
                    assert_eq!(actions.len(), 1);
                    assert!(matches!(&actions[0], Action::SwitchConnection(_)));
                }
                _ => panic!("expected DispatchActions, got {:?}", effects[0]),
            }
        }

        #[test]
        fn is_noop_when_no_last_connection() {
            let mut state = AppState::new("test".to_string());
            state.set_connections(vec![make_profile("conn")]);
            let services = AppServices::stub();

            let action = Action::SwitchToLastConnection;
            let result = reduce_connection_lifecycle(
                &mut state,
                &action,
                Instant::now(),
                &services,
            );

            // Should be handled but produce no effects
            let effects = result.into_effects().expect("should be handled");
            assert!(effects.is_empty(), "should produce no effects");
        }

        #[test]
        fn is_noop_when_last_connection_not_in_list() {
            let mut state = AppState::new("test".to_string());
            // Set a different connection's ID as the last_connection
            let conn = make_profile("conn");
            let other_id = ConnectionId::new();
            state.set_connections(vec![conn]);
            state.set_last_connection_id(Some(other_id.clone()));
            let services = AppServices::stub();

            let action = Action::SwitchToLastConnection;
            let result = reduce_connection_lifecycle(
                &mut state,
                &action,
                Instant::now(),
                &services,
            );

            // Should be handled but produce no effects
            let effects = result.into_effects().expect("should be handled");
            assert!(effects.is_empty(), "should produce no effects");
        }
    }
}
