use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use super::app_config_file::{
    self, config_file_path, get_config_dir as app_config_dir, render_config_file, write_config_file,
};
use crate::app::ports::outbound::connection_store::{ConnectionStore, ConnectionStoreError};
use crate::config::connection_config::{CURRENT_VERSION, ConfigVersionCheck, ConnectionConfigFile};
use crate::domain::connection::{ConnectionId, ConnectionName, ConnectionProfile};

#[cfg(test)]
use super::app_config_file::CONFIG_FILE_NAME;
#[cfg(test)]
use std::path::Path;

pub struct TomlConnectionStore {
    config_dir: PathBuf,
}

impl TomlConnectionStore {
    pub fn new() -> Result<Self, ConnectionStoreError> {
        let config_dir = get_config_dir()?;
        Ok(Self { config_dir })
    }

    pub fn with_config_dir(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    /// Given a source profile and existing profiles, produce a new profile
    /// with a duplicated name that uses the next available number suffix.
    ///
    /// Naming rules:
    /// - "gumshoe" → "gumshoe (1)"
    /// - "gumshoe (1)" → "gumshoe (2)"  
    /// - "gumshoe" when "gumshoe (1)" exists → "gumshoe (2)" etc.
    fn make_duplicated_name(source: &ConnectionProfile, existing: &[ConnectionProfile]) -> ConnectionProfile {
        let source_name = source.display_name();
        let (base_name, _) = Self::parse_name_suffix(source_name);

        // Gather all numbers used by connections with this base name
        let all_numbers = Self::gather_suffix_numbers(existing, &base_name);
        let max_existing = all_numbers.iter().max().copied().unwrap_or(0);

        // Start from the next number after the max
        let mut next_num = if all_numbers.is_empty() { 1 } else { max_existing + 1 };

        // Check that this number doesn't already exist, increment if it does
        while Self::has_name_with_number_suffix(existing, &base_name, next_num) {
            next_num += 1;
        }

        let new_name = format!("{base_name} ({next_num})");
        ConnectionProfile {
            id: source.id.clone(), // temporary, will be replaced
            name: ConnectionName::new(new_name).unwrap(),
            host: source.host.clone(),
            port: source.port,
            database: source.database.clone(),
            username: source.username.clone(),
            password: source.password.clone(),
            ssl_mode: source.ssl_mode,
        }
    }

    /// Parse a connection name to extract the base name and any number suffix.
    /// e.g. "gumshoe (17)" → ("gumshoe", vec![17])
    ///      "gumshoe" → ("gumshoe", vec![])
    fn parse_name_suffix(name: &str) -> (String, Vec<usize>) {
        // Look for a suffix like " (N)" at the end
        if let Some(paren_pos) = name.rfind(" (") {
            let after_space = &name[paren_pos + 2..];
            if after_space.ends_with(')') && after_space.len() > 1 {
                let num_str = &after_space[..after_space.len() - 1];
                if let Ok(num) = num_str.parse::<usize>() {
                    let base = name[..paren_pos].to_string();
                    return (base, vec![num]);
                }
            }
        }
        (name.to_string(), vec![])
    }

    /// Gather all numbers from profiles whose base name matches.
    fn gather_suffix_numbers(profiles: &[ConnectionProfile], base: &str) -> Vec<usize> {
        profiles
            .iter()
            .filter_map(|p| {
                let (b, nums) = Self::parse_name_suffix(p.display_name());
                if b == base {
                    nums.into_iter().next()
                } else {
                    None
                }
            })
            .collect()
    }

    fn has_name_with_number_suffix(profiles: &[ConnectionProfile], base: &str, num: usize) -> bool {
        let expected = format!("{base} ({num})");
        profiles.iter().any(|p| p.display_name() == expected)
    }

    fn config_file_path(&self) -> PathBuf {
        config_file_path(&self.config_dir)
    }

    fn load_config_file(&self) -> Result<Option<ConnectionConfigFile>, ConnectionStoreError> {
        let path = self.config_file_path();
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)?;
        let version_check: ConfigVersionCheck = toml::from_str(&content)?;

        if version_check.version != CURRENT_VERSION {
            return Err(ConnectionStoreError::VersionMismatch {
                found: version_check.version,
                expected: CURRENT_VERSION,
            });
        }

        Ok(Some(toml::from_str::<ConnectionConfigFile>(&content)?))
    }

    fn write_all(&self, profiles: &[ConnectionProfile]) -> Result<(), ConnectionStoreError> {
        let mut config = ConnectionConfigFile::from(profiles);
        if let Some(existing_config) = self.load_config_file()? {
            config.theme = existing_config.theme;
            config.keymap_preset = existing_config.keymap_preset;
            config.er_browser = existing_config.er_browser;
            config.low_scroll_allow_horizontal_scroll =
                existing_config.low_scroll_allow_horizontal_scroll;
            config.low_scroll_max_lines_per_row = existing_config.low_scroll_max_lines_per_row;
        }
        let content = toml::to_string_pretty(&config)?;
        let content_with_header = render_config_file(&content);
        write_config_file(&self.config_dir, &content_with_header)?;

        Ok(())
    }
}

impl ConnectionStore for TomlConnectionStore {
    fn load(&self) -> Result<Option<ConnectionProfile>, ConnectionStoreError> {
        let profiles = self.load_all()?;
        Ok(profiles.into_iter().next())
    }

    fn load_all(&self) -> Result<Vec<ConnectionProfile>, ConnectionStoreError> {
        let Some(config) = self.load_config_file()? else {
            return Ok(vec![]);
        };

        Vec::<ConnectionProfile>::try_from(&config).map_err(ConnectionStoreError::InvalidProfile)
    }

    fn save(&self, profile: &ConnectionProfile) -> Result<(), ConnectionStoreError> {
        let _guard = app_config_file::lock();
        let mut profiles = self.load_all()?;

        let normalized_name = profile.name.normalized();
        if profiles
            .iter()
            .any(|p| p.id != profile.id && p.name.normalized() == normalized_name)
        {
            return Err(ConnectionStoreError::DuplicateName(
                profile.name.as_str().to_string(),
            ));
        }

        if let Some(pos) = profiles.iter().position(|p| p.id == profile.id) {
            profiles[pos] = profile.clone();
        } else {
            profiles.push(profile.clone());
        }

        self.write_all(&profiles)
    }

    fn find_by_id(
        &self,
        id: &ConnectionId,
    ) -> Result<Option<ConnectionProfile>, ConnectionStoreError> {
        let profiles = self.load_all()?;
        Ok(profiles.into_iter().find(|p| &p.id == id))
    }

    fn delete(&self, id: &ConnectionId) -> Result<(), ConnectionStoreError> {
        let _guard = app_config_file::lock();
        let mut profiles = self.load_all()?;
        let original_len = profiles.len();
        profiles.retain(|p| &p.id != id);

        if profiles.len() == original_len {
            return Err(ConnectionStoreError::NotFound(id.to_string()));
        }

        self.write_all(&profiles)
    }

    fn duplicate(
        &self,
        id: &ConnectionId,
    ) -> Result<ConnectionProfile, ConnectionStoreError> {
        let _guard = app_config_file::lock();
        let profiles = self.load_all()?;

        let source = profiles
            .iter()
            .find(|p| &p.id == id)
            .ok_or_else(|| ConnectionStoreError::NotFound(id.to_string()))?;

        let new_profile = Self::make_duplicated_name(source, &profiles);
        let new_id = crate::domain::connection::ConnectionId::new();

        let final_profile = ConnectionProfile::with_id(
            new_id,
            new_profile.name.as_str(),
            new_profile.host.clone(),
            new_profile.port,
            new_profile.database.clone(),
            new_profile.username.clone(),
            new_profile.password.clone(),
            new_profile.ssl_mode,
        ).map_err(|e| ConnectionStoreError::InvalidProfile(e.into()))?;

        let mut all_profiles = profiles;
        all_profiles.push(final_profile.clone());
        self.write_all(&all_profiles)?;

        Ok(final_profile)
    }

    fn storage_path(&self) -> PathBuf {
        self.config_file_path()
    }

    fn save_last_connection_id(&self, id: &ConnectionId) -> Result<(), ConnectionStoreError> {
        let path = app_config_file::last_connection_file_path(&self.config_dir);
        // Write to a temp file first for atomicity
        let tmp_path = path.with_extension(format!("{}.tmp", id.as_str()));
        if let Err(e) = fs::write(&tmp_path, id.as_str()) {
            let _ = fs::remove_file(&tmp_path);
            return Err(ConnectionStoreError::Io(Arc::new(e)));
        }
        if let Err(e) = fs::rename(&tmp_path, &path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(ConnectionStoreError::Io(Arc::new(e)));
        }
        Ok(())
    }
}

fn get_config_dir() -> Result<PathBuf, ConnectionStoreError> {
    Ok(app_config_dir()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::connection::SslMode;
    use tempfile::TempDir;

    fn make_test_profile(name: &str) -> ConnectionProfile {
        ConnectionProfile::new(
            name,
            "localhost",
            5432,
            "testdb",
            "testuser",
            "testpass",
            SslMode::Prefer,
        )
        .unwrap()
    }

    mod loading {
        use super::*;

        #[test]
        fn no_file_returns_empty_vec() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());

            let result = store.load_all().unwrap();

            assert!(result.is_empty());
        }

        #[test]
        fn reports_version_mismatch_for_v1_format() {
            let temp_dir = TempDir::new().unwrap();
            let config_path = temp_dir.path().join(CONFIG_FILE_NAME);

            let content = r#"
version = 1

[connection]
id = "test-id"
host = "localhost"
port = 5432
database = "testdb"
username = "testuser"
password = "testpass"
ssl_mode = "prefer"
"#;
            fs::write(&config_path, content).unwrap();

            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());
            let result = store.load_all();

            assert!(matches!(
                result,
                Err(ConnectionStoreError::VersionMismatch {
                    found: 1,
                    expected: 2
                })
            ));
        }

        #[test]
        fn reports_error_for_invalid_toml() {
            let temp_dir = TempDir::new().unwrap();
            let config_path = temp_dir.path().join(CONFIG_FILE_NAME);

            fs::write(&config_path, "invalid toml {{{{").unwrap();

            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());
            let result = store.load_all();

            assert!(matches!(
                result,
                Err(ConnectionStoreError::TomlDeserialize(_))
            ));
        }

        #[test]
        fn missing_username_and_blank_host_load_as_empty_strings() {
            let temp_dir = TempDir::new().unwrap();
            let config_path = temp_dir.path().join(CONFIG_FILE_NAME);

            let content = r#"
version = 2

[[connections]]
id = "test-id"
name = "Local"
host = ""
port = 5432
database = "testdb"
password = ""
ssl_mode = "prefer"
"#;
            fs::write(&config_path, content).unwrap();

            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());
            let profiles = store.load_all().unwrap();

            assert_eq!(profiles.len(), 1);
            assert_eq!(profiles[0].username, "");
            assert_eq!(profiles[0].host, "");
        }
    }

    mod save {
        use super::*;

        #[test]
        fn creates_config_directory_if_missing() {
            let temp_dir = TempDir::new().unwrap();
            let config_dir = temp_dir.path().join("nested").join("config");
            let store = TomlConnectionStore::with_config_dir(config_dir.clone());
            let profile = make_test_profile("Test");

            store.save(&profile).unwrap();

            assert!(config_dir.exists());
            assert!(store.storage_path().exists());
        }

        #[test]
        fn duplicate_name_returns_error() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());

            let profile1 = make_test_profile("Production");
            let profile2 = make_test_profile("production"); // case-insensitive match

            store.save(&profile1).unwrap();
            let result = store.save(&profile2);

            assert!(matches!(
                result,
                Err(ConnectionStoreError::DuplicateName(_))
            ));
        }

        #[test]
        fn same_id_updates_without_duplicate_error() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());

            let mut profile = make_test_profile("Production");
            store.save(&profile).unwrap();

            profile.host = "newhost".to_string();
            let result = store.save(&profile);

            assert!(result.is_ok());
        }

        #[test]
        fn preserves_existing_app_settings() {
            let temp_dir = TempDir::new().unwrap();
            let config_path = temp_dir.path().join(CONFIG_FILE_NAME);
            fs::write(
                &config_path,
                "version = 2\ntheme = \"light\"\nkeymap_preset = \"ide\"\ner_browser = \"Firefox\"\nconnections = []\n",
            )
            .unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());
            let profile = make_test_profile("Test");

            store.save(&profile).unwrap();

            let content = fs::read_to_string(config_path).unwrap();
            assert!(content.contains("theme = \"light\""));
            assert!(content.contains("keymap_preset = \"ide\""));
            assert!(content.contains("er_browser = \"Firefox\""));
            assert!(content.contains("[[connections]]"));
        }

        #[cfg(unix)]
        #[test]
        fn sets_permissions_to_0600() {
            use std::os::unix::fs::PermissionsExt;

            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());
            let profile = make_test_profile("Test");

            store.save(&profile).unwrap();

            let path = store.storage_path();
            let metadata = fs::metadata(&path).unwrap();
            let mode = metadata.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    mod delete {
        use super::*;

        #[test]
        fn removes_connection_by_id() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());

            let profile = make_test_profile("Test");
            store.save(&profile).unwrap();

            store.delete(&profile.id).unwrap();

            assert!(store.load_all().unwrap().is_empty());
        }

        #[test]
        fn nonexistent_id_returns_not_found() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());

            let result = store.delete(&ConnectionId::new());

            assert!(matches!(result, Err(ConnectionStoreError::NotFound(_))));
        }
    }

    mod lookup {
        use super::*;

        #[test]
        fn existing_id_finds_connection() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());

            let profile = make_test_profile("Test");
            store.save(&profile).unwrap();

            let found = store.find_by_id(&profile.id).unwrap();

            assert!(found.is_some());
            assert_eq!(found.unwrap().name.as_str(), "Test");
        }

        #[test]
        fn missing_id_returns_none() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());

            let found = store.find_by_id(&ConnectionId::new()).unwrap();

            assert!(found.is_none());
        }
    }

    mod roundtrip {
        use super::*;

        #[test]
        fn save_and_load_preserves_data() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());
            let profile = make_test_profile("Test Connection");

            store.save(&profile).unwrap();
            let loaded = store.load().unwrap();

            assert!(loaded.is_some());
            let loaded = loaded.unwrap();
            assert_eq!(loaded.name.as_str(), profile.name.as_str());
            assert_eq!(loaded.host, profile.host);
            assert_eq!(loaded.port, profile.port);
            assert_eq!(loaded.database, profile.database);
            assert_eq!(loaded.username, profile.username);
            assert_eq!(loaded.password, profile.password);
            assert_eq!(loaded.ssl_mode, profile.ssl_mode);
        }
    }

    mod storage_path {
        use super::*;

        #[test]
        fn matches_config_file_path() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());

            let path = store.storage_path();

            assert_eq!(path, temp_dir.path().join(CONFIG_FILE_NAME));
        }
    }

    mod version_mismatch {
        use super::*;

        #[test]
        fn save_returns_error_instead_of_losing_data() {
            let temp_dir = TempDir::new().unwrap();
            let config_path = temp_dir.path().join(CONFIG_FILE_NAME);

            let v1_content = r#"
version = 1

[connection]
id = "test-id"
host = "localhost"
port = 5432
database = "testdb"
username = "testuser"
password = "testpass"
ssl_mode = "prefer"
"#;
            fs::write(&config_path, v1_content).unwrap();

            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());
            let profile = make_test_profile("New Connection");
            let result = store.save(&profile);

            assert!(matches!(
                result,
                Err(ConnectionStoreError::VersionMismatch {
                    found: 1,
                    expected: 2
                })
            ));

            let content_after = fs::read_to_string(&config_path).unwrap();
            assert!(content_after.contains("version = 1"));
        }
    }

    mod atomic_write {
        use super::*;

        #[test]
        fn leaves_no_temp_file() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());
            let profile = make_test_profile("Test");

            store.save(&profile).unwrap();

            let tmp_files: Vec<_> = fs::read_dir(temp_dir.path())
                .unwrap()
                .flatten()
                .filter(|e| {
                    e.file_name().to_str().is_some_and(|n| {
                        Path::new(n)
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
                    })
                })
                .collect();
            assert!(tmp_files.is_empty());
        }

        #[test]
        fn existing_file_preserved_on_save_roundtrip() {
            let temp_dir = TempDir::new().unwrap();
            let store = TomlConnectionStore::with_config_dir(temp_dir.path().to_path_buf());

            let profile1 = make_test_profile("First");
            let mut profile2 = make_test_profile("Second");
            store.save(&profile1).unwrap();
            store.save(&profile2).unwrap();

            profile2.host = "updated-host".to_string();
            store.save(&profile2).unwrap();

            let all = store.load_all().unwrap();
            assert_eq!(all.len(), 2);
            assert!(all.iter().any(|p| p.name.as_str() == "First"));
            assert!(
                all.iter()
                    .any(|p| p.name.as_str() == "Second" && p.host == "updated-host")
            );
        }
    }
}
