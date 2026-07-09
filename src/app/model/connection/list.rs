#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionListItem {
    /// Top panel: last connection (if any)
    LastConnection,
    /// Bottom panel: regular profile connections
    Profile(usize),
    /// Bottom panel: pg_service.conf entries
    Service(usize),
}

pub fn is_service_selected(items: &[ConnectionListItem], selected: usize) -> bool {
    matches!(items.get(selected), Some(ConnectionListItem::Service(_)))
}

pub fn is_last_connection_selected(items: &[ConnectionListItem], selected: usize) -> bool {
    matches!(items.get(selected), Some(ConnectionListItem::LastConnection))
}

pub fn has_last_connection(items: &[ConnectionListItem]) -> bool {
    matches!(items.first(), Some(ConnectionListItem::LastConnection))
}

pub fn build_connection_list(
    profile_count: usize,
    service_count: usize,
    last_connection: bool,
) -> Vec<ConnectionListItem> {
    let mut items = Vec::new();

    if last_connection {
        items.push(ConnectionListItem::LastConnection);
    }

    for i in 0..profile_count {
        items.push(ConnectionListItem::Profile(i));
    }

    for i in 0..service_count {
        items.push(ConnectionListItem::Service(i));
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_profiles_and_services_concatenated() {
        let items = build_connection_list(2, 3);

        assert_eq!(
            items,
            vec![
                ConnectionListItem::Profile(0),
                ConnectionListItem::Profile(1),
                ConnectionListItem::Service(0),
                ConnectionListItem::Service(1),
                ConnectionListItem::Service(2),
            ]
        );
    }

    #[test]
    fn only_profiles_no_separator() {
        let items = build_connection_list(2, 0);

        assert_eq!(
            items,
            vec![
                ConnectionListItem::Profile(0),
                ConnectionListItem::Profile(1),
            ]
        );
    }

    #[test]
    fn only_services_no_separator() {
        let items = build_connection_list(0, 2);

        assert_eq!(
            items,
            vec![
                ConnectionListItem::Service(0),
                ConnectionListItem::Service(1),
            ]
        );
    }

    #[test]
    fn both_empty_returns_empty() {
        let items = build_connection_list(0, 0);

        assert!(items.is_empty());
    }
}
