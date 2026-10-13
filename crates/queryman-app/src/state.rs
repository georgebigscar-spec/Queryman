//! Состояние UI. Меняется только в UI-потоке.

use queryman_core::model::{Environment, Request, ResponseData, Variable, Workspace, WorkspaceSnapshot};
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
}

impl Tab {
    fn new(id: TabId, request: Request) -> Self {
        Self { id, request, section: 0, response_section: 0, pretty_view: true, response: None, in_flight: None }
    }
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
}
