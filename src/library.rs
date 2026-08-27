//! Favorites, recently played games, and play history.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::catalog::manifest::GameId;
use crate::error::StorageError;
use crate::paths::{quarantine, read_json, write_json_atomic};

pub const LIBRARY_SCHEMA_VERSION: u32 = 1;
const MAX_RECENT: usize = 25;
const MAX_SESSIONS: usize = 5000;

/// How a game process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ExitOutcome {
    Code { code: i32 },
    Signal { signal: i32 },
    Unknown,
}

impl ExitOutcome {
    pub fn success(&self) -> bool {
        matches!(self, ExitOutcome::Code { code: 0 })
    }

    pub fn label(&self) -> String {
        match self {
            ExitOutcome::Code { code } => format!("exit code {code}"),
            ExitOutcome::Signal { signal } => format!("terminated by signal {signal}"),
            ExitOutcome::Unknown => "unknown exit status".into(),
        }
    }
}

/// One play session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaySession {
    pub game: GameId,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub duration_secs: u64,
    pub exit: ExitOutcome,
}

impl PlaySession {
    pub fn duration(&self) -> Duration {
        Duration::from_secs(self.duration_secs)
    }
}

/// Aggregated statistics for one game.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayStats {
    pub sessions: usize,
    pub total: Duration,
    pub last_played: Option<DateTime<Utc>>,
    pub last_exit: Option<ExitOutcome>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LibraryFile {
    #[serde(default = "schema")]
    schema_version: u32,
    #[serde(default)]
    favorites: BTreeSet<String>,
    #[serde(default)]
    recent: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HistoryFile {
    #[serde(default = "schema")]
    schema_version: u32,
    #[serde(default)]
    sessions: Vec<PlaySession>,
}

fn schema() -> u32 {
    LIBRARY_SCHEMA_VERSION
}

/// Persistent per-user library state.
#[derive(Debug)]
pub struct Library {
    library_path: PathBuf,
    history_path: PathBuf,
    favorites: BTreeSet<GameId>,
    recent: Vec<GameId>,
    sessions: Vec<PlaySession>,
}

impl Library {
    /// Load both files; corrupt files are quarantined (returned) and replaced with empty state.
    pub fn load(
        library_path: &Path,
        history_path: &Path,
    ) -> Result<(Library, Vec<PathBuf>), StorageError> {
        let mut quarantined = Vec::new();
        let lib = match read_json::<LibraryFile>(library_path) {
            Ok(Some(f)) => f,
            Ok(None) => LibraryFile::default(),
            Err(StorageError::Corrupt { .. }) => {
                quarantined.extend(quarantine(library_path));
                LibraryFile::default()
            }
            Err(e) => return Err(e),
        };
        let hist = match read_json::<HistoryFile>(history_path) {
            Ok(Some(f)) => f,
            Ok(None) => HistoryFile::default(),
            Err(StorageError::Corrupt { .. }) => {
                quarantined.extend(quarantine(history_path));
                HistoryFile::default()
            }
            Err(e) => return Err(e),
        };
        let favorites = lib
            .favorites
            .iter()
            .filter_map(|s| GameId::new(s.as_str()).ok())
            .collect();
        let recent = lib
            .recent
            .iter()
            .filter_map(|s| GameId::new(s.as_str()).ok())
            .collect();
        Ok((
            Library {
                library_path: library_path.to_path_buf(),
                history_path: history_path.to_path_buf(),
                favorites,
                recent,
                sessions: hist.sessions,
            },
            quarantined,
        ))
    }

    pub fn is_favorite(&self, id: &GameId) -> bool {
        self.favorites.contains(id)
    }

    /// Toggle and persist; returns the new state.
    pub fn toggle_favorite(&mut self, id: &GameId) -> Result<bool, StorageError> {
        let now_favorite = if self.favorites.remove(id) {
            false
        } else {
            self.favorites.insert(id.clone());
            true
        };
        self.save_library()?;
        Ok(now_favorite)
    }

    pub fn favorites(&self) -> Vec<GameId> {
        self.favorites.iter().cloned().collect()
    }

    /// Most recently played first.
    pub fn recent(&self, limit: usize) -> Vec<GameId> {
        self.recent.iter().take(limit).cloned().collect()
    }

    /// Record a finished session (updates recents + history) and persist.
    pub fn record_session(&mut self, session: PlaySession) -> Result<(), StorageError> {
        self.recent.retain(|g| g != &session.game);
        self.recent.insert(0, session.game.clone());
        self.recent.truncate(MAX_RECENT);
        self.sessions.push(session);
        if self.sessions.len() > MAX_SESSIONS {
            let excess = self.sessions.len() - MAX_SESSIONS;
            self.sessions.drain(..excess);
        }
        self.save_library()?;
        self.save_history()
    }

    pub fn sessions(&self, game: Option<&GameId>) -> Vec<&PlaySession> {
        self.sessions
            .iter()
            .filter(|s| game.is_none_or(|g| &s.game == g))
            .collect()
    }

    pub fn stats(&self, game: &GameId) -> PlayStats {
        let mut stats = PlayStats::default();
        for s in self.sessions.iter().filter(|s| &s.game == game) {
            stats.sessions += 1;
            stats.total += s.duration();
            if stats.last_played.is_none_or(|t| s.ended_at > t) {
                stats.last_played = Some(s.ended_at);
                stats.last_exit = Some(s.exit);
            }
        }
        stats
    }

    pub fn total_play_time(&self) -> Duration {
        self.sessions.iter().map(PlaySession::duration).sum()
    }

    fn save_library(&self) -> Result<(), StorageError> {
        let file = LibraryFile {
            schema_version: LIBRARY_SCHEMA_VERSION,
            favorites: self.favorites.iter().map(ToString::to_string).collect(),
            recent: self.recent.iter().map(ToString::to_string).collect(),
        };
        write_json_atomic(&self.library_path, &file)
    }

    fn save_history(&self) -> Result<(), StorageError> {
        let file = HistoryFile {
            schema_version: LIBRARY_SCHEMA_VERSION,
            sessions: self.sessions.clone(),
        };
        write_json_atomic(&self.history_path, &file)
    }
}

/// Format a duration as `1h 02m`, `12m 05s`, or `8s`.
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Human-friendly relative time ("3 minutes ago").
pub fn format_relative(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - at).num_seconds().max(0);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        86_400..=2_591_999 => format!("{}d ago", secs / 86_400),
        _ => at.format("%Y-%m-%d").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    fn session(game: &str, secs: u64, code: i32, offset_secs: i64) -> PlaySession {
        let ended = Utc::now() + TimeDelta::seconds(offset_secs);
        PlaySession {
            game: game.parse().unwrap(),
            started_at: ended - TimeDelta::seconds(secs as i64),
            ended_at: ended,
            duration_secs: secs,
            exit: ExitOutcome::Code { code },
        }
    }

    #[test]
    fn favorites_toggle_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let (lib_p, hist_p) = (
            dir.path().join("library.json"),
            dir.path().join("history.json"),
        );
        let (mut lib, q) = Library::load(&lib_p, &hist_p).unwrap();
        assert!(q.is_empty());
        let id: GameId = "chess-tui".parse().unwrap();
        assert!(lib.toggle_favorite(&id).unwrap());
        assert!(lib.is_favorite(&id));
        let (lib2, _) = Library::load(&lib_p, &hist_p).unwrap();
        assert_eq!(lib2.favorites(), vec![id.clone()]);
        let mut lib2 = lib2;
        assert!(!lib2.toggle_favorite(&id).unwrap());
        assert!(lib2.favorites().is_empty());
    }

    #[test]
    fn history_stats_recent_ordering() {
        let dir = tempfile::tempdir().unwrap();
        let (lib_p, hist_p) = (
            dir.path().join("library.json"),
            dir.path().join("history.json"),
        );
        let (mut lib, _) = Library::load(&lib_p, &hist_p).unwrap();
        lib.record_session(session("alpha", 60, 0, -300)).unwrap();
        lib.record_session(session("beta", 30, 1, -200)).unwrap();
        lib.record_session(session("alpha", 90, 0, -100)).unwrap();
        assert_eq!(
            lib.recent(10)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
        let stats = lib.stats(&"alpha".parse().unwrap());
        assert_eq!(stats.sessions, 2);
        assert_eq!(stats.total, Duration::from_secs(150));
        assert!(stats.last_exit.unwrap().success());
        assert_eq!(lib.total_play_time(), Duration::from_secs(180));
        assert_eq!(lib.sessions(Some(&"beta".parse().unwrap())).len(), 1);
        assert_eq!(lib.sessions(None).len(), 3);
        let (lib2, _) = Library::load(&lib_p, &hist_p).unwrap();
        assert_eq!(lib2.sessions(None).len(), 3);
        assert_eq!(lib2.recent(1).len(), 1);
    }

    #[test]
    fn corrupt_files_are_quarantined() {
        let dir = tempfile::tempdir().unwrap();
        let (lib_p, hist_p) = (
            dir.path().join("library.json"),
            dir.path().join("history.json"),
        );
        std::fs::write(&lib_p, "garbage").unwrap();
        std::fs::write(&hist_p, "[1,2").unwrap();
        let (lib, q) = Library::load(&lib_p, &hist_p).unwrap();
        assert_eq!(q.len(), 2);
        assert!(lib.favorites().is_empty());
        assert!(lib.sessions(None).is_empty());
    }

    #[test]
    fn formatting_helpers() {
        assert_eq!(format_duration(Duration::from_secs(8)), "8s");
        assert_eq!(format_duration(Duration::from_secs(125)), "2m 05s");
        assert_eq!(format_duration(Duration::from_secs(3720)), "1h 02m");
        let now = Utc::now();
        assert_eq!(format_relative(now, now), "just now");
        assert_eq!(
            format_relative(now - TimeDelta::minutes(5), now),
            "5 min ago"
        );
        assert_eq!(format_relative(now - TimeDelta::hours(3), now), "3h ago");
        assert_eq!(format_relative(now - TimeDelta::days(2), now), "2d ago");
        assert_eq!(
            ExitOutcome::Signal { signal: 9 }.label(),
            "terminated by signal 9"
        );
    }
}
