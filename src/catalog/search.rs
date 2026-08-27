//! Text search and ranking over manifests.

use super::manifest::GameManifest;

/// Relevance rank for `query` against a manifest (lower is better); `None` = no match.
pub fn rank(m: &GameManifest, query: &str) -> Option<u8> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(3);
    }
    let id = m.id.as_str().to_lowercase();
    let name = m.name.to_lowercase();
    if id == q || name == q {
        return Some(0);
    }
    if id.starts_with(&q) || name.starts_with(&q) {
        return Some(1);
    }
    if id.contains(&q) || name.contains(&q) {
        return Some(2);
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let text = m.search_text();
    if words.iter().all(|w| text.contains(w)) {
        return Some(3);
    }
    None
}

/// Filter and sort manifests by relevance to `query`, then by name.
pub fn search<'a>(
    items: impl IntoIterator<Item = &'a GameManifest>,
    query: &str,
) -> Vec<&'a GameManifest> {
    let mut ranked: Vec<(u8, &GameManifest)> = items
        .into_iter()
        .filter_map(|m| rank(m, query).map(|r| (r, m)))
        .collect();
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
    });
    ranked.into_iter().map(|(_, m)| m).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::manifest::tests::FULL;

    fn make(id: &str, name: &str, summary: &str, tags: &[&str]) -> GameManifest {
        let mut m = GameManifest::parse(FULL).unwrap();
        m.id = id.parse().unwrap();
        m.name = name.into();
        m.summary = summary.into();
        m.tags = tags.iter().map(|t| t.to_string()).collect();
        m
    }

    #[test]
    fn ranks_exact_prefix_contains_text() {
        let chess = make("chess-tui", "Chess TUI", "Play chess.", &["lichess"]);
        let tetro = make("tetro-tui", "Tetro TUI", "Stack tetrominoes.", &["tetris"]);
        let lights = make(
            "rusty-lights",
            "Rusty Lights",
            "Lights out puzzle featuring chess-like logic.",
            &[],
        );
        let all = [&chess, &tetro, &lights];
        assert_eq!(rank(&chess, "chess-tui"), Some(0));
        assert_eq!(rank(&chess, "che"), Some(1));
        assert_eq!(rank(&chess, "ess"), Some(2));
        assert_eq!(rank(&chess, "lichess"), Some(3));
        assert_eq!(rank(&tetro, "chess"), None);
        let found = search(all.iter().copied(), "chess");
        assert_eq!(
            found.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["chess-tui", "rusty-lights"]
        );
        assert_eq!(search(all.iter().copied(), "").len(), 3);
        assert_eq!(search(all.iter().copied(), "tetris puzzle").len(), 1);
    }
}
