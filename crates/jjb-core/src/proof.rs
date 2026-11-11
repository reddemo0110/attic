// SPDX-License-Identifier: GPL-3.0-only

//! Proofreading: spelling and grammar, one line at a time, on top of
//! Harper (offline, English only). The editor asks for each line it
//! shows and caches the answer per line text; the app decides which kind
//! it wants to see.
//!
//! The engine (a dictionary plus the rule set) takes a few hundred
//! milliseconds to build, so it is built once, lazily, by [`load`] —
//! call that from a background thread when a toggle turns on. Until it
//! is there, [`check_line`] answers `None` rather than an empty list, so
//! a caller never caches "nothing wrong" for a line that was never read.

use harper_core::linting::{LintGroup, LintKind, Linter, Suggestion};
use harper_core::parsers::{Markdown, MarkdownOptions};
use harper_core::spell::FstDictionary;
use harper_core::{Dialect, Document};
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};

/// What a found problem is about: the two toggles the user has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Spelling,
    Grammar,
}

/// Which kinds the caller wants reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Want {
    pub spelling: bool,
    pub grammar: bool,
}

impl Want {
    pub const NONE: Want = Want {
        spelling: false,
        grammar: false,
    };

    #[must_use]
    pub fn any(self) -> bool {
        self.spelling || self.grammar
    }
}

/// One problem on a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Byte range within the line.
    pub range: Range<usize>,
    pub kind: Kind,
    /// Harper's one-line explanation.
    pub message: String,
    /// Replacements for the range, best first (an empty one means "delete").
    pub suggestions: Vec<String>,
}

struct Engine {
    dict: Arc<FstDictionary>,
    linter: LintGroup,
}

static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();

/// The English we check against. Australian: "colour", "organise",
/// "favourite" pass; the American spellings are flagged.
const DIALECT: Dialect = Dialect::Australian;

/// Rules in Harper's curated set that fight the way notes are written.
const RULES_OFF: &[&str] = &[
    // "Try to use title case in headings" — a note's headings are sentences.
    "UseTitleCase",
];

/// Build the engine if it is not built yet. Slow the first time (a few
/// hundred ms); instant after. Safe to call from any thread.
pub fn load() {
    ENGINE.get_or_init(|| {
        let dict = FstDictionary::curated();
        let mut linter = LintGroup::new_curated(Arc::clone(&dict), DIALECT);
        for rule in RULES_OFF {
            linter.config.set_rule_enabled(*rule, false);
        }
        Mutex::new(Engine { dict, linter })
    });
}

/// Whether [`load`] has finished.
#[must_use]
pub fn ready() -> bool {
    ENGINE.get().is_some()
}

/// Lines the checker never reads: Attic's own machinery inside the text.
#[must_use]
pub fn skip_line(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<!-- jjb:table") || t.starts_with("<!--")
}

/// Check one line of a note (markdown). `None` while the engine is not
/// loaded; otherwise the issues of the wanted kinds, in text order.
pub fn check_line(text: &str, want: Want) -> Option<Vec<Issue>> {
    if !want.any() || text.trim().is_empty() || skip_line(text) {
        return Some(Vec::new());
    }
    let engine = ENGINE.get()?;
    let mut engine = engine.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Engine { dict, linter } = &mut *engine;
    let doc = Document::new(text, &Markdown::new(MarkdownOptions::default()), &**dict);
    let mut lints = linter.lint(&doc);
    harper_core::remove_overlaps(&mut lints);
    // Char offsets → byte offsets, once per line.
    let byte_at: Vec<usize> = text
        .char_indices()
        .map(|(b, _)| b)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut out: Vec<Issue> = lints
        .into_iter()
        .filter_map(|l| {
            let kind = classify(l.lint_kind);
            let wanted = match kind {
                Kind::Spelling => want.spelling,
                Kind::Grammar => want.grammar,
            };
            if !wanted {
                return None;
            }
            let start = *byte_at.get(l.span.start)?;
            let end = *byte_at.get(l.span.end)?;
            if start >= end {
                return None;
            }
            let suggestions = l
                .suggestions
                .iter()
                .filter_map(|s| match s {
                    Suggestion::ReplaceWith(chars) => Some(chars.iter().collect()),
                    Suggestion::Remove => Some(String::new()),
                    Suggestion::InsertAfter(_) => None,
                })
                .collect();
            Some(Issue {
                range: start..end,
                kind,
                message: l.message,
                suggestions,
            })
        })
        .collect();
    out.sort_by_key(|i| i.range.start);
    Some(out)
}

fn classify(kind: LintKind) -> Kind {
    match kind {
        LintKind::Spelling | LintKind::Typo => Kind::Spelling,
        _ => Kind::Grammar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: Want = Want {
        spelling: true,
        grammar: true,
    };

    fn flagged(text: &str, want: Want) -> Vec<(String, Kind)> {
        load();
        check_line(text, want)
            .expect("engine loaded")
            .into_iter()
            .map(|i| (text[i.range].to_owned(), i.kind))
            .collect()
    }

    #[test]
    fn not_ready_answers_none_without_caching_hazard() {
        // Whether or not another test loaded the engine, a blank line is
        // always an answer: there is nothing to check.
        assert_eq!(check_line("   ", BOTH), Some(Vec::new()));
        assert_eq!(check_line("x", Want::NONE), Some(Vec::new()));
    }

    #[test]
    fn spelling_and_grammar_are_told_apart() {
        let found = flagged("Thiss is an test.", BOTH);
        assert!(found.contains(&("Thiss".to_owned(), Kind::Spelling)));
        assert!(found.contains(&("an".to_owned(), Kind::Grammar)));
        let only_spelling = flagged(
            "Thiss is an test.",
            Want {
                spelling: true,
                grammar: false,
            },
        );
        assert_eq!(only_spelling, vec![("Thiss".to_owned(), Kind::Spelling)]);
        let only_grammar = flagged(
            "Thiss is an test.",
            Want {
                spelling: false,
                grammar: true,
            },
        );
        assert_eq!(only_grammar, vec![("an".to_owned(), Kind::Grammar)]);
    }

    #[test]
    fn australian_spellings_pass() {
        assert!(flagged("I organised the favourite colour analysis.", BOTH).is_empty());
    }

    #[test]
    fn attic_syntax_is_left_alone() {
        assert!(flagged("- [ ] Buy milk #groceries and see [[Shopping list]]", BOTH).is_empty());
        assert!(flagged("## Meeting notes with `cargo build` and https://example.com/x", BOTH).is_empty());
        assert!(flagged("<!-- jjb:table cols=120,0 rows=0,40 -->", BOTH).is_empty());
    }

    #[test]
    fn ranges_are_bytes_even_after_wide_chars() {
        let text = "Café — brocolli";
        let found = flagged(text, BOTH);
        assert_eq!(found, vec![("brocolli".to_owned(), Kind::Spelling)]);
        load();
        let issue = &check_line(text, BOTH).unwrap()[0];
        assert_eq!(issue.suggestions.first().map(String::as_str), Some("broccoli"));
    }
}
