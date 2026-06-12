//! Shared fuzzy-ranking used by every fuzzy picker in the UI (org/project
//! picker, tags editor, iteration filter, help search). Centralising it keeps a
//! single matcher and a single scoring/sort convention across the app.

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

/// Rank `items` against `query`, keeping only matches, best score first.
///
/// `keys` maps an item to one or more candidate strings to match against; the
/// item's score is the best score across its keys (so a multi-field item — e.g.
/// an iteration's name *and* path — matches on any field). An empty or
/// whitespace-only query returns every item in its original order.
pub fn rank<'a, T, F, I, S>(items: &'a [T], query: &str, keys: F) -> Vec<&'a T>
where
    F: Fn(&'a T) -> I,
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if query.trim().is_empty() {
        return items.iter().collect();
    }
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, &T)> = items
        .iter()
        .filter_map(|it| {
            keys(it)
                .into_iter()
                .filter_map(|k| matcher.fuzzy_match(k.as_ref(), query))
                .max()
                .map(|s| (s, it))
        })
        .collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored.into_iter().map(|(_, it)| it).collect()
}

/// True if `hay` fuzzy-matches `query` (whitespace-only query matches anything).
pub fn matches(hay: &str, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    SkimMatcherV2::default().fuzzy_match(hay, query).is_some()
}
