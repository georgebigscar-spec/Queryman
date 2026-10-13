//! Состояние UI. Меняется только в UI-потоке.

use queryman_core::collection::{Collection, Folder, HistoryEntry, Item};
use queryman_core::model::{Auth, Environment, Request, ResponseData, Variable, Workspace, WorkspaceSnapshot};
use queryman_core::protocol::{HttpError, ResponseText, TabId};
use std::time::Instant;
use uuid::Uuid;

pub enum TabResponse {
    Ok { data: ResponseData, text: ResponseText },
    Err(HttpError),
}

pub struct Tab {
    pub id: TabId,
    pub request: Request,
    /// Params / Headers / Body / Auth / Settings
    pub section: usize,
    /// Body / Headers ответа
    pub response_section: usize,
    pub pretty_view: bool,
    pub response: Option<TabResponse>,
    pub in_flight: Option<Instant>,
    /// Запрос в том виде, в каком он был отправлен, — для записи в историю.
    pub sent: Option<(Request, u64)>,
}

impl Tab {
    fn new(id: TabId, request: Request) -> Self {
        Self {
            id,
            request,
            section: 0,
            response_section: 0,
            pretty_view: true,
            response: None,
            in_flight: None,
            sent: None,
        }
    }
}

/// Узел дерева коллекций.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node {
    Collection(Uuid),
    Folder(Uuid),
    Request(Uuid),
}

impl Node {
    pub fn id(self) -> Uuid {
        match self {
            Node::Collection(id) | Node::Folder(id) | Node::Request(id) => id,
        }
    }
}

/// Открытый модальный диалог; одновременно не больше одного.
pub enum Dialog {
    Rename {
        node: Node,
        name: String,
    },
    Delete {
        node: Node,
        title: String,
    },
    EditAuth {
        node: Node,
        auth: Auth,
    },
    /// Сохранение вкладки в коллекцию. `destination` — индекс в [`AppState::destinations`],
    /// `None` — новая коллекция с именем `new_collection`.
    SaveRequest {
        tab: TabId,
        name: String,
        destination: Option<usize>,
        new_collection: String,
    },
    ClearHistory,
}

/// Куда можно сохранить запрос: коллекция и, возможно, папка в ней.
#[derive(Debug, Clone, PartialEq)]
pub struct Destination {
    pub collection: Uuid,
    pub folder: Option<Uuid>,
    pub label: String,
}

pub struct AppState {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub environments: Vec<Environment>,
    pub active_env: Option<Uuid>,
    pub globals: Vec<Variable>,
    pub show_env_editor: bool,
    /// `None` — глобальные переменные, `Some(i)` — окружение i.
    pub env_editor_selection: Option<usize>,
    pub collections: Vec<Collection>,
    pub history: Vec<HistoryEntry>,
    /// Коллекции / История
    pub sidebar_section: usize,
    pub tree_selection: Option<Node>,
    pub dialog: Option<Dialog>,
    next_tab_id: TabId,
}

impl AppState {
    pub fn from_snapshot(snapshot: WorkspaceSnapshot) -> Self {
        let mut state = Self {
            tabs: Vec::new(),
            active: 0,
            active_env: snapshot.workspace.active_env.filter(|id| snapshot.environments.iter().any(|e| e.id == *id)),
            environments: snapshot.environments,
            globals: snapshot.globals,
            show_env_editor: false,
            env_editor_selection: None,
            collections: snapshot.collections,
            history: snapshot.history,
            sidebar_section: 0,
            tree_selection: None,
            dialog: None,
            next_tab_id: 1,
        };
        for request in snapshot.workspace.tabs {
            state.open_tab(request);
        }
        if state.tabs.is_empty() {
            state.open_tab(Request::default());
        }
        state.active = snapshot.workspace.active_tab.min(state.tabs.len() - 1);
        state
    }

    pub fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            workspace: Workspace {
                tabs: self.tabs.iter().map(|t| t.request.clone()).collect(),
                active_tab: self.active,
                active_env: self.active_env,
            },
            environments: self.environments.clone(),
            globals: self.globals.clone(),
            collections: self.collections.clone(),
            history: self.history.clone(),
        }
    }

    pub fn open_tab(&mut self, request: Request) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(Tab::new(id, request));
        self.active = self.tabs.len() - 1;
    }

    pub fn close_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.tabs.remove(index);
        }
        if self.tabs.is_empty() {
            self.open_tab(Request::default());
        }
        if self.active >= self.tabs.len() || (index < self.active) {
            self.active = self.active.saturating_sub(1).min(self.tabs.len() - 1);
        }
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn active_environment(&self) -> Option<&Environment> {
        self.active_env.and_then(|id| self.environments.iter().find(|e| e.id == id))
    }

    pub fn any_in_flight(&self) -> bool {
        self.tabs.iter().any(|t| t.in_flight.is_some())
    }

    pub fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// Сохранённая в коллекции версия запроса.
    pub fn saved_request(&self, id: Uuid) -> Option<&Request> {
        self.collections.iter().find_map(|c| c.find_request(id))
    }

    /// Вкладка связана с запросом коллекции, и в ней есть несохранённые правки.
    pub fn is_dirty(&self, tab: &Tab) -> bool {
        self.saved_request(tab.request.id)
            .is_some_and(|saved| saved.without_blank_rows() != tab.request.without_blank_rows())
    }

    /// Авторизация для режима Inherit; вне коллекций — `None`.
    pub fn inherited_auth(&self, request: Uuid) -> Option<Auth> {
        self.collections.iter().find_map(|c| c.inherited_auth(request))
    }

    fn collection_of(&mut self, id: Uuid) -> Option<&mut Collection> {
        self.collections.iter_mut().find(|c| c.contains(id))
    }

    /// Открывает запрос из коллекции; если он уже открыт, переключается на его вкладку.
    pub fn open_saved(&mut self, id: Uuid) {
        if let Some(i) = self.tabs.iter().position(|t| t.request.id == id) {
            self.active = i;
        } else if let Some(request) = self.saved_request(id).cloned() {
            self.open_tab(request);
        }
    }

    /// Записывает вкладку в коллекцию, если она с ней связана. Возвращает `false`, если нет.
    pub fn save_tab(&mut self, tab: TabId) -> bool {
        let Some(request) = self.tab_index(tab).map(|i| self.tabs[i].request.without_blank_rows()) else {
            return false;
        };
        match self.collections.iter_mut().find_map(|c| c.find_request_mut(request.id)) {
            Some(saved) => {
                *saved = request;
                true
            }
            None => false,
        }
    }

    /// Все коллекции и папки, куда можно сохранить запрос.
    pub fn destinations(&self) -> Vec<Destination> {
        self.collections
            .iter()
            .flat_map(|c| {
                c.containers().into_iter().map(|(folder, label)| Destination { collection: c.id, folder, label })
            })
            .collect()
    }

    /// Сохраняет вкладку как новый запрос в `destination` (или в новую коллекцию).
    pub fn save_tab_as(&mut self, tab: TabId, name: String, destination: Option<Destination>, new_collection: String) {
        let Some(index) = self.tab_index(tab) else { return };
        let (collection, folder) = match destination {
            Some(d) => (d.collection, d.folder),
            None => {
                let name = if new_collection.trim().is_empty() {
                    "Моя коллекция".to_owned()
                } else {
                    new_collection
                };
                let c = Collection { name, ..Default::default() };
                let id = c.id;
                self.collections.push(c);
                (id, None)
            }
        };
        // Новый id: исходная вкладка могла быть связана с другим запросом.
        let request = Request { id: Uuid::new_v4(), name, ..self.tabs[index].request.without_blank_rows() };
        let Some(items) =
            self.collections.iter_mut().find(|c| c.id == collection).and_then(|c| c.container_mut(folder))
        else {
            return;
        };
        items.push(Item::Request(request.clone()));
        self.tabs[index].request = request;
    }

    pub fn add_collection(&mut self) -> Uuid {
        let c = Collection::default();
        let id = c.id;
        self.collections.push(c);
        id
    }

    /// Новая папка или запрос внутри коллекции или папки `parent`.
    pub fn add_child(&mut self, parent: Node, item: Item) {
        let folder = match parent {
            Node::Collection(_) => None,
            Node::Folder(id) => Some(id),
            Node::Request(_) => return,
        };
        let open = match &item {
            Item::Request(r) => Some(r.clone()),
            Item::Folder(_) => None,
        };
        if let Some(items) = self.collection_of(parent.id()).and_then(|c| c.container_mut(folder)) {
            items.push(item);
            if let Some(request) = open {
                self.open_tab(request);
            }
        }
    }

    pub fn node_name(&self, node: Node) -> Option<String> {
        let id = node.id();
        let c = self.collections.iter().find(|c| c.contains(id))?;
        match node {
            Node::Collection(_) => Some(c.name.clone()),
            Node::Request(id) => c.find_request(id).map(|r| r.name.clone()),
            Node::Folder(id) => c.find_folder(id).map(|f| f.name.clone()),
        }
    }

    pub fn rename(&mut self, node: Node, name: String) {
        match node {
            Node::Collection(id) => {
                if let Some(c) = self.collections.iter_mut().find(|c| c.id == id) {
                    c.name = name;
                }
            }
            Node::Folder(id) => {
                if let Some(f) = self.collection_of(id).and_then(|c| c.find_folder_mut(id)) {
                    f.name = name;
                }
            }
            Node::Request(id) => {
                if let Some(r) = self.collection_of(id).and_then(|c| c.find_request_mut(id)) {
                    r.name = name.clone();
                }
                // Открытая вкладка переименовывается вместе с запросом.
                for tab in self.tabs.iter_mut().filter(|t| t.request.id == id) {
                    tab.request.name = name.clone();
                }
            }
        }
    }

    pub fn node_auth(&self, node: Node) -> Option<Auth> {
        match node {
            Node::Collection(id) => self.collections.iter().find(|c| c.id == id).map(|c| c.auth.clone()),
            Node::Folder(id) => self.collections.iter().find_map(|c| c.find_folder(id)).map(|f| f.auth.clone()),
            Node::Request(_) => None,
        }
    }

    pub fn set_node_auth(&mut self, node: Node, auth: Auth) {
        match node {
            Node::Collection(id) => {
                if let Some(c) = self.collections.iter_mut().find(|c| c.id == id) {
                    c.auth = auth;
                }
            }
            Node::Folder(id) => {
                if let Some(f) = self.collection_of(id).and_then(|c| c.find_folder_mut(id)) {
                    f.auth = auth;
                }
            }
            Node::Request(_) => {}
        }
    }

    pub fn duplicate(&mut self, node: Node) {
        let id = node.id();
        let Some(c) = self.collection_of(id) else { return };
        match node {
            Node::Collection(_) => {
                let copy = Collection {
                    id: Uuid::new_v4(),
                    name: format!("{} (копия)", c.name),
                    auth: c.auth.clone(),
                    items: c.items.iter().map(Collection::duplicate).collect(),
                };
                self.collections.push(copy);
            }
            Node::Folder(_) | Node::Request(_) => {
                duplicate_in(&mut c.items, id);
            }
        }
    }

    pub fn delete(&mut self, node: Node) {
        match node {
            Node::Collection(id) => self.collections.retain(|c| c.id != id),
            Node::Folder(id) | Node::Request(id) => {
                if let Some(c) = self.collection_of(id) {
                    c.remove(id);
                }
            }
        }
        if self.tree_selection == Some(node) {
            self.tree_selection = None;
        }
    }

    pub fn record_history(&mut self, entry: HistoryEntry) {
        queryman_core::collection::push_history(&mut self.history, entry);
    }
}

/// Вставляет копию элемента `id` сразу после оригинала.
fn duplicate_in(items: &mut Vec<Item>, id: Uuid) -> bool {
    if let Some(pos) = items.iter().position(|item| item.id() == id) {
        let mut copy = Collection::duplicate(&items[pos]);
        match &mut copy {
            Item::Request(r) => r.name = format!("{} (копия)", r.name),
            Item::Folder(f) => f.name = format!("{} (копия)", f.name),
        }
        items.insert(pos + 1, copy);
        return true;
    }
    items.iter_mut().any(|item| match item {
        Item::Folder(Folder { items, .. }) => duplicate_in(items, id),
        Item::Request(_) => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        AppState::from_snapshot(WorkspaceSnapshot::default())
    }

    #[test]
    fn save_as_links_tab_and_tracks_changes() {
        let mut s = state();
        let tab = s.active_tab().id;
        s.active_tab_mut().request.url = "example.com".into();
        assert!(!s.save_tab(tab), "несвязанная вкладка сохраняется только через диалог");

        s.save_tab_as(tab, "Пользователи".into(), None, "API".into());
        assert_eq!(s.collections.len(), 1);
        assert_eq!(s.collections[0].name, "API");
        let id = s.active_tab().request.id;
        assert_eq!(s.saved_request(id).unwrap().name, "Пользователи");
        assert!(!s.is_dirty(s.active_tab()));

        s.active_tab_mut().request.url = "example.com/v2".into();
        assert!(s.is_dirty(s.active_tab()));
        assert!(s.save_tab(tab));
        assert!(!s.is_dirty(s.active_tab()));
        assert_eq!(s.saved_request(id).unwrap().url, "example.com/v2");
    }

    #[test]
    fn open_saved_reuses_tab_and_rename_updates_it() {
        let mut s = state();
        let c = s.add_collection();
        let request = Request { name: "a".into(), ..Default::default() };
        let id = request.id;
        s.add_child(Node::Collection(c), Item::Request(request));
        assert_eq!(s.tabs.len(), 2);

        s.active = 0;
        s.open_saved(id);
        assert_eq!(s.tabs.len(), 2);
        assert_eq!(s.active, 1);

        s.rename(Node::Request(id), "b".into());
        assert_eq!(s.active_tab().request.name, "b");
        assert!(!s.is_dirty(s.active_tab()));
    }

    #[test]
    fn duplicate_and_delete_folder() {
        let mut s = state();
        let c = s.add_collection();
        let folder = Folder::default();
        let folder_id = folder.id;
        s.add_child(Node::Collection(c), Item::Folder(folder));
        s.duplicate(Node::Folder(folder_id));
        assert_eq!(s.collections[0].items.len(), 2);
        s.delete(Node::Folder(folder_id));
        assert_eq!(s.collections[0].items.len(), 1);
        assert_ne!(s.collections[0].items[0].id(), folder_id);
    }
}
