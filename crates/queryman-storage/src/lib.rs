//! Хранение данных в JSON-файлах (раздел 7 спецификации).
//!
//! ```text
//! <root>/
//! ├── workspace.json      открытые вкладки и черновики, активное окружение
//! ├── globals.json        глобальные переменные
//! ├── history.json        история запросов без тел ответов
//! ├── environments/<uuid>.json
//! └── collections/<uuid>.json
//! ```
//! Каждый файл содержит `schema_version`. Запись атомарная: временный файл + rename.

use queryman_core::collection::{Collection, HistoryEntry, HISTORY_LIMIT};
use queryman_core::model::{Environment, Variable, Workspace, WorkspaceSnapshot};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;
const APP_DIR: &str = "queryman";

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("не удалось определить каталог данных пользователя")]
    NoDataDir,
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {source}")]
    Json { path: PathBuf, source: serde_json::Error },
}

/// Результат загрузки. Повреждённые файлы не роняют приложение:
/// они переименовываются в `*.corrupt`, а пользователь видит предупреждение.
#[derive(Debug, Default)]
pub struct LoadReport {
    pub snapshot: WorkspaceSnapshot,
    pub warnings: Vec<String>,
}

#[derive(Deserialize)]
struct Versioned<T> {
    schema_version: u32,
    #[serde(flatten)]
    data: T,
}

#[derive(Serialize)]
struct VersionedRef<'a, T> {
    schema_version: u32,
    #[serde(flatten)]
    data: &'a T,
}

#[derive(Default, Serialize, Deserialize)]
struct Globals {
    #[serde(default)]
    variables: Vec<Variable>,
}

#[derive(Default, Serialize, Deserialize)]
struct History {
    #[serde(default)]
    entries: Vec<HistoryEntry>,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Каталог по умолчанию или портативный режим: если рядом с exe лежит файл
    /// `portable`, данные хранятся в `./data` рядом с ним.
    pub fn default_root() -> Result<PathBuf, StorageError> {
        if let Some(dir) = std::env::current_exe().ok().as_deref().and_then(Path::parent) {
            if dir.join("portable").exists() {
                return Ok(dir.join("data"));
            }
        }
        directories::ProjectDirs::from("", "", APP_DIR)
            .map(|d| d.data_dir().to_path_buf())
            .ok_or(StorageError::NoDataDir)
    }

    pub fn open_default() -> Result<Self, StorageError> {
        Self::open(Self::default_root()?)
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        for dir in ["environments", "collections"] {
            let path = root.join(dir);
            fs::create_dir_all(&path).map_err(|source| StorageError::Io { path, source })?;
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn workspace_path(&self) -> PathBuf {
        self.root.join("workspace.json")
    }

    fn globals_path(&self) -> PathBuf {
        self.root.join("globals.json")
    }

    fn history_path(&self) -> PathBuf {
        self.root.join("history.json")
    }

    fn environments_dir(&self) -> PathBuf {
        self.root.join("environments")
    }

    fn collections_dir(&self) -> PathBuf {
        self.root.join("collections")
    }

    pub fn load(&self) -> LoadReport {
        let mut warnings = Vec::new();
        let workspace: Workspace = read_versioned(&self.workspace_path(), &mut warnings).unwrap_or_default();
        let globals: Globals = read_versioned(&self.globals_path(), &mut warnings).unwrap_or_default();
        let mut history: History = read_versioned(&self.history_path(), &mut warnings).unwrap_or_default();
        history.entries.truncate(HISTORY_LIMIT);

        let mut environments: Vec<Environment> =
            read_dir_versioned(&self.environments_dir(), "окружения", &mut warnings);
        environments.sort_by_key(|e| e.name.to_lowercase());
        let mut collections: Vec<Collection> = read_dir_versioned(&self.collections_dir(), "коллекции", &mut warnings);
        collections.sort_by_key(|c| c.name.to_lowercase());

        LoadReport {
            snapshot: WorkspaceSnapshot {
                workspace,
                environments,
                globals: globals.variables,
                collections,
                history: history.entries,
            },
            warnings,
        }
    }

    pub fn save(&self, snapshot: &WorkspaceSnapshot) -> Result<(), StorageError> {
        let mut workspace = snapshot.workspace.clone();
        workspace.tabs = workspace.tabs.iter().map(|t| t.without_blank_rows()).collect();
        write_versioned(&self.workspace_path(), &workspace)?;

        let variables = snapshot.globals.iter().filter(|v| !v.is_blank()).cloned().collect();
        write_versioned(&self.globals_path(), &Globals { variables })?;

        write_versioned(&self.history_path(), &History { entries: snapshot.history.clone() })?;

        let environments = snapshot.environments.iter().map(|env| {
            let mut env = env.clone();
            env.variables.retain(|v| !v.is_blank());
            (env.id, env)
        });
        write_dir_versioned(&self.environments_dir(), environments)?;

        let collections = snapshot.collections.iter().map(|c| (c.id, strip_collection(c)));
        write_dir_versioned(&self.collections_dir(), collections)
    }
}

/// Копия коллекции без пустых строк-заготовок в запросах.
fn strip_collection(collection: &Collection) -> Collection {
    use queryman_core::collection::Item;
    fn strip(items: &[Item]) -> Vec<Item> {
        items
            .iter()
            .map(|item| match item {
                Item::Request(r) => Item::Request(r.without_blank_rows()),
                Item::Folder(f) => {
                    let mut f = f.clone();
                    f.items = strip(&f.items);
                    Item::Folder(f)
                }
            })
            .collect()
    }
    Collection { items: strip(&collection.items), ..collection.clone() }
}

/// Читает все `*.json` каталога; повреждённые файлы пропускаются с предупреждением.
fn read_dir_versioned<T: DeserializeOwned>(dir: &Path, what: &str, warnings: &mut Vec<String>) -> Vec<T> {
    let mut out = Vec::new();
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    if let Some(item) = read_versioned::<T>(&path, warnings) {
                        out.push(item);
                    }
                }
            }
        }
        Err(e) => warnings.push(format!("Не удалось прочитать {what}: {e}")),
    }
    out
}

/// Пишет каждый элемент в `<uuid>.json` и удаляет файлы элементов, которых больше нет.
fn write_dir_versioned<T: Serialize>(
    dir: &Path,
    items: impl Iterator<Item = (uuid::Uuid, T)>,
) -> Result<(), StorageError> {
    let mut keep = HashSet::new();
    for (id, item) in items {
        let file_name = format!("{id}.json");
        write_versioned(&dir.join(&file_name), &item)?;
        keep.insert(file_name);
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") && !keep.contains(&name) {
                if let Err(e) = fs::remove_file(entry.path()) {
                    tracing::warn!("не удалось удалить {name}: {e}");
                }
            }
        }
    }
    Ok(())
}

fn read_versioned<T: DeserializeOwned>(path: &Path, warnings: &mut Vec<String>) -> Option<T> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            warnings.push(format!("Не удалось прочитать {}: {e}", path.display()));
            return None;
        }
    };
    match serde_json::from_slice::<Versioned<T>>(&bytes) {
        Ok(v) if v.schema_version <= SCHEMA_VERSION => Some(v.data),
        Ok(v) => {
            warnings.push(format!(
                "{} создан более новой версией Queryman (схема {}), файл пропущен",
                path.display(),
                v.schema_version
            ));
            None
        }
        Err(e) => {
            let mut backup = path.as_os_str().to_owned();
            backup.push(".corrupt");
            let backup = PathBuf::from(backup);
            let note = match fs::rename(path, &backup) {
                Ok(()) => format!("сохранён как {}", backup.display()),
                Err(_) => "переименовать не удалось".to_owned(),
            };
            warnings.push(format!("Файл {} повреждён ({e}); {note}", path.display()));
            None
        }
    }
}

fn write_versioned<T: Serialize>(path: &Path, data: &T) -> Result<(), StorageError> {
    let value = VersionedRef { schema_version: SCHEMA_VERSION, data };
    let bytes =
        serde_json::to_vec_pretty(&value).map_err(|source| StorageError::Json { path: path.to_owned(), source })?;
    atomic_write(path, &bytes)
}

/// Сбой посреди записи не портит исходный файл (NFR-11).
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let io_err = |source| StorageError::Io { path: path.to_owned(), source };
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(io_err)?;
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{file_name}.tmp"));
    {
        let mut file = fs::File::create(&tmp).map_err(io_err)?;
        file.write_all(bytes).map_err(io_err)?;
        file.sync_all().map_err(io_err)?;
    }
    fs::rename(&tmp, path).map_err(io_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use queryman_core::collection::Item;
    use queryman_core::model::{KeyValue, Request};

    fn sample() -> WorkspaceSnapshot {
        let env = Environment {
            name: "dev".into(),
            variables: vec![Variable::new("host", "localhost")],
            ..Default::default()
        };
        let mut tab = Request { url: "{{host}}/users".into(), ..Default::default() };
        tab.headers = vec![KeyValue::new("Accept", "application/json"), KeyValue::default()];
        WorkspaceSnapshot {
            workspace: Workspace { tabs: vec![tab], active_tab: 0, active_env: Some(env.id) },
            environments: vec![env],
            globals: vec![Variable::new("token", "t"), Variable::default()],
            collections: vec![Collection {
                name: "api".into(),
                items: vec![Item::Request(Request {
                    headers: vec![KeyValue::new("X", "1"), KeyValue::default()],
                    ..Default::default()
                })],
                ..Default::default()
            }],
            history: vec![HistoryEntry { status: Some(200), ..Default::default() }],
        }
    }

    #[test]
    fn roundtrip_drops_blank_rows() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let snap = sample();
        store.save(&snap).unwrap();

        let report = store.load();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let loaded = report.snapshot;
        assert_eq!(loaded.environments, snap.environments);
        assert_eq!(loaded.globals.len(), 1);
        assert_eq!(loaded.workspace.tabs[0].headers.len(), 1);
        assert_eq!(loaded.workspace.active_env, snap.workspace.active_env);
        assert_eq!(loaded.history, snap.history);
        let Item::Request(r) = &loaded.collections[0].items[0] else { panic!() };
        assert_eq!(r.headers.len(), 1);
    }

    #[test]
    fn deleted_collection_file_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut snap = sample();
        store.save(&snap).unwrap();
        assert_eq!(fs::read_dir(dir.path().join("collections")).unwrap().count(), 1);
        snap.collections.clear();
        store.save(&snap).unwrap();
        assert_eq!(fs::read_dir(dir.path().join("collections")).unwrap().count(), 0);
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        fs::write(dir.path().join("workspace.json"), "{ broken").unwrap();
        let report = store.load();
        assert_eq!(report.warnings.len(), 1);
        assert!(dir.path().join("workspace.json.corrupt").exists());
        assert!(report.snapshot.workspace.tabs.is_empty());
    }

    #[test]
    fn deleted_environment_file_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut snap = sample();
        store.save(&snap).unwrap();
        snap.environments.clear();
        store.save(&snap).unwrap();
        assert_eq!(fs::read_dir(dir.path().join("environments")).unwrap().count(), 0);
    }

    #[test]
    fn atomic_write_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        atomic_write(&path, b"1").unwrap();
        atomic_write(&path, b"2").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"2");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
