//! Коллекции, папки и история запросов (FR-30, FR-31, FR-33).

use crate::model::{Auth, Request};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Коллекция: дерево папок и запросов. Хранится в `collections/<uuid>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Collection {
    pub id: Uuid,
    pub name: String,
    /// Авторизация для запросов в режиме Inherit. `Inherit` здесь означает «без авторизации».
    pub auth: Auth,
    pub items: Vec<Item>,
}

impl Default for Collection {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(), name: "Новая коллекция".to_owned(), auth: Auth::None, items: Vec::new()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    pub auth: Auth,
    pub items: Vec<Item>,
}

impl Default for Folder {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), name: "Новая папка".to_owned(), auth: Auth::Inherit, items: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Item {
    Folder(Folder),
    Request(Request),
}

impl Item {
    pub fn id(&self) -> Uuid {
        match self {
            Item::Folder(f) => f.id,
            Item::Request(r) => r.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Item::Folder(f) => &f.name,
            Item::Request(r) => &r.name,
        }
    }
}

impl Collection {
    pub fn find_request(&self, id: Uuid) -> Option<&Request> {
        find_request(&self.items, id)
    }

    pub fn find_request_mut(&mut self, id: Uuid) -> Option<&mut Request> {
        find_request_mut(&mut self.items, id)
    }

    pub fn find_folder(&self, id: Uuid) -> Option<&Folder> {
        find_folder(&self.items, id)
    }

    pub fn find_folder_mut(&mut self, id: Uuid) -> Option<&mut Folder> {
        find_folder_mut(&mut self.items, id)
    }

    /// Содержимое коллекции (`folder == None`) или папки.
    pub fn container_mut(&mut self, folder: Option<Uuid>) -> Option<&mut Vec<Item>> {
        match folder {
            None => Some(&mut self.items),
            Some(id) => self.find_folder_mut(id).map(|f| &mut f.items),
        }
    }

    pub fn contains(&self, id: Uuid) -> bool {
        self.id == id || contains(&self.items, id)
    }

    /// Удаляет папку или запрос вместе с содержимым.
    pub fn remove(&mut self, id: Uuid) -> Option<Item> {
        remove(&mut self.items, id)
    }

    /// Авторизация, которую унаследует запрос `id` в режиме Inherit:
    /// ближайшая папка с собственной авторизацией, иначе авторизация коллекции.
    /// `None`, если запроса в коллекции нет.
    pub fn inherited_auth(&self, id: Uuid) -> Option<Auth> {
        let mut chain = Vec::new();
        if !path_to(&self.items, id, &mut chain) {
            return None;
        }
        let from_folder = chain.iter().rev().find(|f| f.auth != Auth::Inherit).map(|f| f.auth.clone());
        Some(from_folder.unwrap_or_else(|| match &self.auth {
            Auth::Inherit => Auth::None,
            auth => auth.clone(),
        }))
    }

    /// Все места, куда можно сохранить запрос: коллекция и её папки, с путём для показа.
    pub fn containers(&self) -> Vec<(Option<Uuid>, String)> {
        let mut out = vec![(None, self.name.clone())];
        collect_folders(&self.items, &self.name, &mut out);
        out
    }

    /// Все запросы коллекции вместе с путём к их папке (`Коллекция / Папка`), для поиска.
    pub fn requests(&self) -> Vec<(String, &Request)> {
        let mut out = Vec::new();
        collect_requests(&self.items, &self.name, &mut out);
        out
    }

    /// Копия элемента с новыми идентификаторами (для «Дублировать»).
    pub fn duplicate(item: &Item) -> Item {
        match item {
            Item::Request(r) => Item::Request(Request { id: Uuid::new_v4(), ..r.clone() }),
            Item::Folder(f) => Item::Folder(Folder {
                id: Uuid::new_v4(),
                name: f.name.clone(),
                auth: f.auth.clone(),
                items: f.items.iter().map(Self::duplicate).collect(),
            }),
        }
    }
}

fn find_request(items: &[Item], id: Uuid) -> Option<&Request> {
    items.iter().find_map(|item| match item {
        Item::Request(r) if r.id == id => Some(r),
        Item::Request(_) => None,
        Item::Folder(f) => find_request(&f.items, id),
    })
}

fn find_request_mut(items: &mut [Item], id: Uuid) -> Option<&mut Request> {
    items.iter_mut().find_map(|item| match item {
        Item::Request(r) if r.id == id => Some(r),
        Item::Request(_) => None,
        Item::Folder(f) => find_request_mut(&mut f.items, id),
    })
}

fn find_folder(items: &[Item], id: Uuid) -> Option<&Folder> {
    items.iter().find_map(|item| match item {
        Item::Folder(f) if f.id == id => Some(f),
        Item::Folder(f) => find_folder(&f.items, id),
        Item::Request(_) => None,
    })
}

fn find_folder_mut(items: &mut [Item], id: Uuid) -> Option<&mut Folder> {
    items.iter_mut().find_map(|item| match item {
        Item::Folder(f) => {
            if f.id == id {
                Some(f)
            } else {
                find_folder_mut(&mut f.items, id)
            }
        }
        Item::Request(_) => None,
    })
}

fn contains(items: &[Item], id: Uuid) -> bool {
    items.iter().any(|item| {
        item.id() == id
            || match item {
                Item::Folder(f) => contains(&f.items, id),
                Item::Request(_) => false,
            }
    })
}

fn remove(items: &mut Vec<Item>, id: Uuid) -> Option<Item> {
    if let Some(pos) = items.iter().position(|item| item.id() == id) {
        return Some(items.remove(pos));
    }
    items.iter_mut().find_map(|item| match item {
        Item::Folder(f) => remove(&mut f.items, id),
        Item::Request(_) => None,
    })
}

/// Заполняет `chain` папками от корня до элемента `id`.
fn path_to<'a>(items: &'a [Item], id: Uuid, chain: &mut Vec<&'a Folder>) -> bool {
    for item in items {
        match item {
            Item::Request(r) if r.id == id => return true,
            Item::Request(_) => {}
            Item::Folder(f) => {
                if f.id == id {
                    return true;
                }
                chain.push(f);
                if path_to(&f.items, id, chain) {
                    return true;
                }
                chain.pop();
            }
        }
    }
    false
}

fn collect_requests<'a>(items: &'a [Item], prefix: &str, out: &mut Vec<(String, &'a Request)>) {
    for item in items {
        match item {
            Item::Request(r) => out.push((prefix.to_owned(), r)),
            Item::Folder(f) => collect_requests(&f.items, &format!("{prefix} / {}", f.name), out),
        }
    }
}

fn collect_folders(items: &[Item], prefix: &str, out: &mut Vec<(Option<Uuid>, String)>) {
    for item in items {
        if let Item::Folder(f) = item {
            let path = format!("{prefix} / {}", f.name);
            out.push((Some(f.id), path.clone()));
            collect_folders(&f.items, &path, out);
        }
    }
}

/// Запись истории. Тело ответа не сохраняется.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryEntry {
    pub id: Uuid,
    /// Время отправки, миллисекунды Unix.
    pub sent_at_ms: u64,
    /// Запрос до подстановки переменных — в таком виде его удобно повторить.
    pub request: Request,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub elapsed_ms: Option<u64>,
}

impl Default for HistoryEntry {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            sent_at_ms: 0,
            request: Request::default(),
            status: None,
            error: None,
            elapsed_ms: None,
        }
    }
}

/// Сколько записей истории хранится.
pub const HISTORY_LIMIT: usize = 500;

/// Добавляет запись в начало истории и обрезает её до [`HISTORY_LIMIT`].
pub fn push_history(history: &mut Vec<HistoryEntry>, entry: HistoryEntry) {
    history.insert(0, entry);
    history.truncate(HISTORY_LIMIT);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bearer(token: &str) -> Auth {
        Auth::Bearer { token: token.into(), prefix: String::new() }
    }

    fn sample() -> (Collection, Uuid, Uuid, Uuid) {
        let top = Request { name: "top".into(), ..Default::default() };
        let inner = Request { name: "inner".into(), ..Default::default() };
        let deep = Request { name: "deep".into(), ..Default::default() };
        let (top_id, inner_id, deep_id) = (top.id, inner.id, deep.id);
        let sub = Folder { name: "sub".into(), items: vec![Item::Request(deep)], ..Default::default() };
        let folder = Folder {
            name: "f".into(),
            auth: bearer("folder"),
            items: vec![Item::Request(inner), Item::Folder(sub)],
            ..Default::default()
        };
        let c = Collection {
            name: "api".into(),
            auth: bearer("collection"),
            items: vec![Item::Request(top), Item::Folder(folder)],
            ..Default::default()
        };
        (c, top_id, inner_id, deep_id)
    }

    #[test]
    fn inherited_auth_uses_nearest_folder_then_collection() {
        let (c, top, inner, deep) = sample();
        assert_eq!(c.inherited_auth(top), Some(bearer("collection")));
        assert_eq!(c.inherited_auth(inner), Some(bearer("folder")));
        assert_eq!(c.inherited_auth(deep), Some(bearer("folder")));
        assert_eq!(c.inherited_auth(Uuid::new_v4()), None);

        let plain = Collection { auth: Auth::Inherit, ..c.clone() };
        assert_eq!(plain.inherited_auth(top), Some(Auth::None));
    }

    #[test]
    fn find_remove_and_containers() {
        let (mut c, top, _, deep) = sample();
        assert_eq!(c.find_request(deep).unwrap().name, "deep");
        assert!(c.contains(deep));
        let paths: Vec<String> = c.containers().into_iter().map(|(_, p)| p).collect();
        assert_eq!(paths, ["api", "api / f", "api / f / sub"]);

        assert!(c.remove(deep).is_some());
        assert!(!c.contains(deep));
        assert!(c.find_request(top).is_some());
    }

    #[test]
    fn requests_with_paths() {
        let (c, ..) = sample();
        let found: Vec<(String, &str)> = c.requests().into_iter().map(|(p, r)| (p, r.name.as_str())).collect();
        assert_eq!(found, [("api".into(), "top"), ("api / f".into(), "inner"), ("api / f / sub".into(), "deep")]);
    }

    #[test]
    fn duplicate_assigns_new_ids() {
        let (c, ..) = sample();
        let copy = Collection::duplicate(&c.items[1]);
        assert_ne!(copy.id(), c.items[1].id());
        let Item::Folder(f) = copy else { panic!() };
        assert!(!c.contains(f.items[0].id()));
    }

    #[test]
    fn collection_roundtrips_through_json() {
        let (c, ..) = sample();
        let json = serde_json::to_string(&c).unwrap();
        let back: Collection = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn history_is_capped() {
        let mut history = Vec::new();
        for i in 0..HISTORY_LIMIT + 5 {
            push_history(&mut history, HistoryEntry { sent_at_ms: i as u64, ..Default::default() });
        }
        assert_eq!(history.len(), HISTORY_LIMIT);
        assert_eq!(history[0].sent_at_ms, (HISTORY_LIMIT + 4) as u64);
    }
}
