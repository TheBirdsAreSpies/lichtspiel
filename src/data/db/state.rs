//! The whole library in memory (`Db`): its parts, its defaults, reading older versions and keeping it consistent.

use super::*;

impl Db {
    /// Older versions kept sign-in tokens as they were given out; keep only their hash (they still work).
    pub fn hash_old_tokens(&mut self) {
        for t in self.tokens.iter_mut().filter(|t| t.token.len() != 64) { t.token = crate::accounts::auth::token_hash(&t.token); }
    }

    /// Brings a library saved by an older version up to date (at start and after a restore).
    pub fn upgrade(&mut self) {
        if self.ticket_key.is_empty() { self.ticket_key = random_hex(32); }
        self.hash_old_tokens();
    }

    /// Credits markers of an item: set by hand (extra.credits_ms), else from
    /// chapters, but only when credits markers are generated at all (server
    /// setting not "never" and the library's credits detection on).
    pub fn credits_markers(&self, it: &Item) -> Vec<u64> {
        if let Some(a) = it.extra.get("credits_ms").and_then(|v| v.as_array()) {
            return a.iter().filter_map(|v| v.as_u64()).collect();
        }
        let library_on = self.credits_on(it);
        if self.settings.credits_markers_behaviour == "never" || !library_on {
            return vec![];
        }
        // Found in the last file; counted from the start of the whole movie.
        let start = it.last_file_start_ms();
        it.last_file().map(|f| f.credits_ms.iter().map(|c| c + start).collect()).unwrap_or_default()
    }

    /// Intro detection for an item: a TV library with "Enable intro
    /// detection" on, and the server setting not "never" (only
    /// schedules it for show libraries).
    pub fn intro_on(&self, it: &Item) -> bool {
        self.settings.intro_markers_behaviour != "never"
            && self.sections.iter().find(|s| s.id == it.section_id).is_some_and(|s| s.kind == SectionKind::Show && s.intro_detection)
    }

    /// An episode's intro (start, end ms): set by hand (extra.intro_ms), else found.
    pub fn intro_marker(&self, it: &Item) -> Option<(u64, u64)> {
        let pair = |v: &[u64]| (v.len() == 2 && v[1] > v[0]).then(|| (v[0], v[1]));
        if let Some(a) = it.extra.get("intro_ms").and_then(|v| v.as_array()) {
            return pair(&a.iter().filter_map(|v| v.as_u64()).collect::<Vec<_>>());
        }
        if it.kind != ItemKind::Episode || !self.intro_on(it) {
            return None;
        }
        it.file.as_ref().and_then(|f| pair(&f.intro_ms))
    }

    /// An episode's recap ("Previously on", start and end ms), when intro markers are made.
    pub fn recap_marker(&self, it: &Item) -> Option<(u64, u64)> {
        if it.kind != ItemKind::Episode || !self.intro_on(it) {
            return None;
        }
        let f = it.file.as_ref()?;
        (f.recap_ms.len() == 2 && f.recap_ms[1] > f.recap_ms[0]).then(|| (f.recap_ms[0], f.recap_ms[1]))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Db {
    pub version: u32,
    pub machine_id: String,
    pub next_id: u64,
    pub settings: Settings,
    pub sections: Vec<Section>,
    pub items: Tracked<u64, Item>,
    pub users: Vec<User>,
    pub invites: Vec<Invite>,
    pub tokens: Vec<Token>,
    pub playlists: Vec<Playlist>,
    /// key "user_id:item_id"
    pub views: Tracked<String, ViewState>,
    pub plugins: BTreeMap<String, PluginState>,
    #[serde(default)]
    pub history: Vec<PlayEvent>,
    /// Plays that have ended, oldest first (see PlaySession).
    #[serde(default)]
    pub sessions: Vec<PlaySession>,
    #[serde(default)]
    pub reports: Vec<ProblemReport>,
    #[serde(default)]
    pub downloads: Vec<Download>,
    /// Signs the short-lived media tickets (auth::ticket); made on first start.
    #[serde(default)]
    pub ticket_key: String,
}

impl Default for Db {
    fn default() -> Self {
        let mut db = Db {
            version: 1,
            machine_id: random_hex(16),
            next_id: 1,
            settings: Settings::default(),
            sections: vec![],
            items: Tracked::default(),
            users: vec![],
            invites: vec![],
            tokens: vec![],
            playlists: vec![],
            views: Tracked::default(),
            plugins: BTreeMap::new(),
            history: vec![],
            sessions: vec![],
            reports: vec![],
            downloads: vec![],
            ticket_key: random_hex(32),
        };
        db.ensure_default_playlists();
        db
    }
}

impl Db {
    /// Reads an older version's JSON library file.
    pub fn load_json(path: &Path) -> Db {
        let mut db = match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Db>(&bytes) {
                Ok(db) => db,
                Err(e) => {
                    let backup = path.with_extension("broken.json");
                    let _ = std::fs::copy(path, &backup);
                    elog!(
                        "library file could not be read ({e}); a copy was kept at {}",
                        backup.display()
                    );
                    Db::default()
                }
            },
            Err(_) => Db::default(),
        };
        db.after_load();
        db
    }

    /// Updates that apply to a library however it was read.
    pub fn after_load(&mut self) {
        // Servers still called by the old default name get the new one; a name
        // chosen by the owner stays.
        if matches!(self.settings.server_name.trim(), "Home Media Server" | "Media Server") {
            self.settings.server_name = crate::server::APP_NAME.into();
        }
        self.ensure_default_playlists();
    }

    pub fn new_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Adds a viewing to the play history.
    /// A watched movie or episode leaves the watchlist; a season or show once
    /// every episode of it is watched.
    pub fn record_play(&mut self, user_id: u64, item_id: u64, manual: bool) {
        self.record_play_at(user_id, item_id, manual, now());
    }

    /// The same for a viewing at another time (imported from elsewhere); the
    /// history stays in time order.
    pub fn record_play_at(&mut self, user_id: u64, item_id: u64, manual: bool, at: u64) {
        let pos = self.history.iter().rposition(|e| e.at <= at).map_or(0, |i| i + 1);
        self.history.insert(pos, PlayEvent { user_id, item_id, at, manual });
        let mut up = vec![item_id];
        let mut at = self.items.get(&item_id).and_then(|i| i.parent_id);
        while let Some(p) = at {
            up.push(p);
            at = self.items.get(&p).and_then(|i| i.parent_id);
        }
        for (n, id) in up.into_iter().enumerate() {
            if self.views.get(&format!("{user_id}:{id}")).is_none_or(|v| v.watchlist_at == 0) {
                continue;
            }
            let done = n == 0 || self.leaves(id).iter().all(|l| *l == item_id || self.view(user_id, *l).view_count > 0);
            if done {
                self.view_mut(user_id, id).watchlist_at = 0;
            }
        }
        if self.history.len() > HISTORY_MAX {
            let extra = self.history.len() - HISTORY_MAX;
            self.history.drain(..extra);
        }
    }

    /// Keeps an ended play for the statistics (when it ran long enough).
    pub fn record_session(&mut self, s: PlaySession) -> bool {
        if s.watched_ms < SESSION_MIN_MS { return false; }
        let pos = self.sessions.iter().rposition(|e| e.at <= s.at).map_or(0, |i| i + 1);
        self.sessions.insert(pos, s);
        if self.sessions.len() > SESSIONS_MAX {
            let extra = self.sessions.len() - SESSIONS_MAX;
            self.sessions.drain(..extra);
        }
        true
    }

    pub fn user(&self, id: u64) -> Option<&User> {
        self.users.iter().find(|u| u.id == id)
    }

    pub fn view(&self, user: u64, item: u64) -> ViewState {
        self.views
            .get(&format!("{user}:{item}"))
            .cloned()
            .unwrap_or_default()
    }

    pub fn view_mut(&mut self, user: u64, item: u64) -> &mut ViewState {
        self.views.entry(format!("{user}:{item}")).or_default()
    }

    pub fn can_see_section(&self, user: &User, section: u64) -> bool {
        user.admin
            || match &user.allowed_sections {
                None => true,
                Some(list) => list.contains(&section),
            }
    }

    /// Library access plus the person's age limit. Seasons and episodes take the
    /// rating of the nearest parent that has one.
    pub fn can_see_item(&self, user: &User, item: &Item) -> bool {
        if !self.can_see_section(user, item.section_id) { return false; }
        let Some(max) = user.max_age else { return true };
        if user.admin { return true; }
        let mut cur = item;
        for _ in 0..4 {
            if !cur.content_rating.trim().is_empty() {
                return match rating_age(&cur.content_rating) {
                    Some(a) => a <= max,
                    None => !user.block_unrated,
                };
            }
            match cur.parent_id.and_then(|p| self.items.get(&p)) { Some(p) => cur = p, None => break }
        }
        !user.block_unrated
    }

    /// A further file of another title that is still there (several versions of one episode or movie):
    /// shown under that title, not as a title or an episode of its own.
    pub fn is_version(&self, i: &Item) -> bool {
        i.version_of().is_some_and(|m| self.items.contains_key(&m))
    }

    pub fn children(&self, parent: u64) -> Vec<&Item> {
        let mut v: Vec<&Item> = self
            .items
            .values()
            .filter(|i| i.parent_id == Some(parent) && !self.is_version(i))
            .collect();
        // Songs of a double album: disc by disc.
        let disc = |i: &Item| i.extra.get("disc").and_then(|d| d.as_u64()).unwrap_or(0);
        v.sort_by(|a, b| {
            disc(a)
                .cmp(&disc(b))
                .then(a.index.unwrap_or(0).cmp(&b.index.unwrap_or(0)))
                .then(a.sort_title.cmp(&b.sort_title))
        });
        v
    }

    /// All playable items below an item (a show → its episodes).
    pub fn leaves(&self, id: u64) -> Vec<u64> {
        let Some(item) = self.items.get(&id) else {
            return vec![];
        };
        if item.kind.playable() {
            return vec![id];
        }
        let mut out = vec![];
        for c in self.children(id) {
            out.extend(self.leaves(c.id));
        }
        out
    }

    /// The default playlists every user gets. They are smart playlists whose
    /// rules are worked out per user (their own watch state).
    pub fn ensure_default_playlists(&mut self) {
        let defaults: Vec<(&str, &str, &str, SmartQuery)> = vec![
            (
                "continue",
                "Continue Watching",
                "Everything you started but did not finish.",
                SmartQuery {
                    match_all: true,
                    rules: vec![rule("in_progress", "is", Value::Bool(true))],
                    sort: "last_viewed_at:desc".into(),
                    limit: Some(50),
                    sections: vec![],
                },
            ),
            (
                "recent_movies",
                "Recently Added Movies",
                "Movies added in the last 30 days.",
                SmartQuery {
                    match_all: true,
                    rules: vec![
                        rule("kind", "is", Value::from("movie")),
                        rule("added_at", "in_last_days", Value::from(30)),
                    ],
                    sort: "added_at:desc".into(),
                    limit: Some(50),
                    sections: vec![],
                },
            ),
            (
                "recent_episodes",
                "Recently Added Episodes",
                "TV episodes added in the last 30 days.",
                SmartQuery {
                    match_all: true,
                    rules: vec![
                        rule("kind", "is", Value::from("episode")),
                        rule("added_at", "in_last_days", Value::from(30)),
                    ],
                    sort: "added_at:desc".into(),
                    limit: Some(50),
                    sections: vec![],
                },
            ),
            (
                "unwatched_movies",
                "Unwatched Movies",
                "Movies you have not seen yet.",
                SmartQuery {
                    match_all: true,
                    rules: vec![
                        rule("kind", "is", Value::from("movie")),
                        rule("watched", "is", Value::Bool(false)),
                    ],
                    sort: "title".into(),
                    limit: None,
                    sections: vec![],
                },
            ),
            (
                "favorites",
                "Favorites",
                "Everything you marked with a heart.",
                SmartQuery {
                    match_all: true,
                    rules: vec![rule("favorite", "is", Value::Bool(true))],
                    sort: "title".into(),
                    limit: None,
                    sections: vec![],
                },
            ),
            (
                "top_rated",
                "Top Rated",
                "Rated 8 or higher.",
                SmartQuery {
                    match_all: true,
                    rules: vec![rule("rating", "gte", Value::from(8))],
                    sort: "rating:desc".into(),
                    limit: Some(100),
                    sections: vec![],
                },
            ),
        ];
        for (key, title, summary, query) in defaults {
            if self
                .playlists
                .iter()
                .any(|p| p.default_key.as_deref() == Some(key))
            {
                continue;
            }
            let id = self.new_id();
            let t = now();
            self.playlists.push(Playlist {
                id,
                owner_id: 0,
                title: title.into(),
                summary: summary.into(),
                kind: PlaylistKind::Smart,
                items: vec![],
                query: Some(query),
                default_key: Some(key.into()),
                shared: true,
                pinned: true,
                created_at: t,
                updated_at: t,
            });
        }
    }
}
