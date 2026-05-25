use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

pub struct PagedSession {
    pub session_id: String,
    pub columns: Vec<String>,
    pub all_rows: Vec<serde_json::Value>,
    pub current_position: usize,
    pub last_accessed: Instant,
}

pub struct SessionStore {
    pub sessions: HashMap<String, PagedSession>,
}

impl SessionStore {
    pub fn new() -> Self {
        SessionStore {
            sessions: HashMap::new(),
        }
    }

    pub fn insert(&mut self, session: PagedSession) {
        self.sessions.insert(session.session_id.clone(), session);
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut PagedSession> {
        self.sessions.get_mut(id)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.sessions.remove(id).is_some()
    }

    /// Expire idle sessions. Returns the number of sessions removed.
    pub fn expire_idle(&mut self, timeout: Duration) -> usize {
        let now = Instant::now();
        let before = self.sessions.len();
        self.sessions
            .retain(|_, s| now.duration_since(s.last_accessed) < timeout);
        before - self.sessions.len()
    }

    /// Clear all sessions.
    pub fn clear_all(&mut self) {
        self.sessions.clear();
    }
}

pub static SESSION_STORE: std::sync::LazyLock<Arc<Mutex<SessionStore>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(SessionStore::new())));

pub const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60); // 10 minutes
