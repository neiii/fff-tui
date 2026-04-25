use fff_search::{
    file_picker::FilePicker, git::format_git_status_opt, FilePickerOptions, FuzzySearchOptions,
    GrepMode, GrepSearchOptions, PaginationArgs, QueryParser, SharedFrecency, SharedPicker,
    SharedQueryTracker,
};
use std::path::PathBuf;

/// Search behaviour toggles (case sensitivity, regex, etc.).
#[derive(Clone, Copy, Debug, Default)]
pub struct SearchMode {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub fixed_strings: bool,
    pub group_grep: bool,
    pub fuzzy: bool,
}

/// Which result types to include in a search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchScope {
    #[default]
    Unified,
    FileOnly,
    GrepOnly,
}

/// The kind of search result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MatchKind {
    #[default]
    File,
    Line,
    FileHeader,
}

/// Unique key for a result used in multi-select.
pub fn selection_key(result: &UnifiedResult) -> String {
    match result.line_number {
        Some(ln) => format!("{}:{}:{}", result.absolute_path, ln, result.column.unwrap_or(1)),
        None => result.absolute_path.clone(),
    }
}

/// A unified search result — either a file path match or a content line match.
#[derive(Clone, Default)]
pub struct UnifiedResult {
    pub kind: MatchKind,
    pub relative_path: String,
    pub absolute_path: String,
    pub score: i32,
    pub exact_match: bool,
    /// Line-only fields
    pub line_number: Option<u64>,
    pub column: Option<u32>,
    pub line_content: Option<String>,
    pub match_byte_offsets: Option<Vec<(u32, u32)>>,
    pub is_definition: Option<bool>,
    pub git_status: Option<String>,
}

/// Output of a search operation.
pub struct SearchOutput {
    pub results: Vec<UnifiedResult>,
    pub highlight_query: String,
    pub fuzzy_total_matched: usize,
    pub grep_page_matched: usize,
}

impl SearchOutput {
    fn empty(query: &str) -> Self {
        Self {
            results: Vec::new(),
            highlight_query: query.to_string(),
            fuzzy_total_matched: 0,
            grep_page_matched: 0,
        }
    }
}

/// Wrapper around fff-core's FilePicker that provides a sync API for the TUI.
#[derive(Clone)]
pub struct PickerBackend {
    shared_picker: SharedPicker,
    shared_frecency: SharedFrecency,
    shared_query_tracker: SharedQueryTracker,
    base_path: PathBuf,
}

impl PickerBackend {
    pub fn new(base_path: &str) -> anyhow::Result<Self> {
        let canonical = std::fs::canonicalize(base_path)?;
        let shared_picker = SharedPicker::default();

        // Try to initialize real frecency + query tracker DBs
        let cache_dir = dirs::cache_dir().map(|d| d.join("fff"));
        let (shared_frecency, shared_query_tracker) = if let Some(ref cache) = cache_dir {
            let frecency_path = cache.join("frecency");
            let query_path = cache.join("queries");
            let frecency = SharedFrecency::default();
            let queries = SharedQueryTracker::default();

            let frecency_ok = std::fs::create_dir_all(&frecency_path)
                .is_ok()
                && fff_search::FrecencyTracker::new(&frecency_path, true)
                    .and_then(|t| frecency.init(t))
                    .is_ok();

            let queries_ok = std::fs::create_dir_all(&query_path)
                .is_ok()
                && fff_search::QueryTracker::new(&query_path, true)
                    .and_then(|t| queries.init(t))
                    .is_ok();

            if frecency_ok && queries_ok {
                (frecency, queries)
            } else {
                (SharedFrecency::noop(), SharedQueryTracker::noop())
            }
        } else {
            (SharedFrecency::noop(), SharedQueryTracker::noop())
        };

        FilePicker::new_with_shared_state(
            shared_picker.clone(),
            shared_frecency.clone(),
            FilePickerOptions {
                base_path: canonical.to_string_lossy().into(),
                watch: true,
                ..Default::default()
            },
        )?;

        Ok(Self {
            shared_picker,
            shared_frecency,
            shared_query_tracker,
            base_path: canonical,
        })
    }

    /// Block until the background filesystem scan finishes (or timeout).
    pub fn wait_for_scan(&self, timeout: std::time::Duration) -> bool {
        self.shared_picker.wait_for_scan(timeout)
    }

    pub fn is_scanning(&self) -> bool {
        self.shared_picker
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|p| p.is_scan_active()))
            .unwrap_or(false)
    }

    pub fn total_files(&self) -> usize {
        self.shared_picker
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|p| p.get_files().len()))
            .unwrap_or(0)
    }

    pub fn track_access(&self, path: &str) {
        let Ok(guard) = self.shared_frecency.read() else {
            return;
        };
        let Some(tracker) = guard.as_ref() else {
            return;
        };
        let _ = tracker.track_access(std::path::Path::new(path));
    }

    pub fn track_query_completion(&self, query: &str, file_path: &str) {
        let Ok(mut guard) = self.shared_query_tracker.write() else {
            return;
        };
        let Some(tracker) = guard.as_mut() else {
            return;
        };
        let _ = tracker.track_query_completion(
            query,
            &self.base_path,
            std::path::Path::new(file_path),
        );
    }

    pub fn get_historical_query(&self, offset: usize) -> Option<String> {
        let guard = self.shared_query_tracker.read().ok()?;
        let tracker = guard.as_ref()?;
        tracker.get_historical_query(&self.base_path, offset).ok().flatten()
    }

    pub fn get_historical_grep_query(&self, offset: usize) -> Option<String> {
        let guard = self.shared_query_tracker.read().ok()?;
        let tracker = guard.as_ref()?;
        tracker.get_historical_grep_query(&self.base_path, offset).ok().flatten()
    }

    /// Perform a unified search (fuzzy file + content grep).
    pub fn search(
        &self,
        query: &str,
        mode: SearchMode,
        scope: SearchScope,
        current_file: Option<&str>,
        force_combo: bool,
        limit: usize,
        time_budget_ms: u64,
    ) -> SearchOutput {
        let guard = match self.shared_picker.read() {
            Ok(g) => g,
            Err(_) => return SearchOutput::empty(query),
        };

        let picker = match guard.as_ref() {
            Some(p) => p,
            None => return SearchOutput::empty(query),
        };

        let parser = QueryParser::default();
        let parsed = parser.parse(query);

        // Build the highlight query from fuzzy parts only (exclude constraints)
        let highlight_query = match &parsed.fuzzy_query {
            fff_search::FuzzyQuery::Text(t) => t.to_string(),
            fff_search::FuzzyQuery::Parts(parts) => parts.join(""),
            fff_search::FuzzyQuery::Empty => String::new(),
        };

        // 1. Fuzzy file search
        let mut exact_files = Vec::new();
        let mut other_files = Vec::new();
        let mut total_matched = 0usize;

        if scope != SearchScope::GrepOnly {
            let qt_guard = match self.shared_query_tracker.read() {
                Ok(g) => g,
                Err(_) => return SearchOutput::empty(query),
            };
            let qt_ref = qt_guard.as_ref();

            let fuzzy_results = picker.fuzzy_search(
                &parsed,
                qt_ref,
                FuzzySearchOptions {
                    max_threads: 0, // auto
                    current_file,
                    project_path: Some(&self.base_path),
                    combo_boost_score_multiplier: if force_combo { 1 } else { 0 },
                    min_combo_count: if force_combo { 0 } else { 0 },
                    pagination: PaginationArgs { offset: 0, limit },
                },
            );

            total_matched = fuzzy_results.total_matched;

            for (item, score) in fuzzy_results.items.iter().zip(fuzzy_results.scores.iter()) {
                let relative_path = item.relative_path(picker);
                let absolute_path = item.absolute_path(picker, &self.base_path);
                let result = UnifiedResult {
                    kind: MatchKind::File,
                    relative_path,
                    absolute_path: absolute_path.to_string_lossy().into_owned(),
                    score: score.total,
                    exact_match: score.exact_match,
                    git_status: format_git_status_opt(item.git_status).map(|s| s.to_string()),
                    ..Default::default()
                };
                if score.exact_match {
                    exact_files.push(result);
                } else {
                    other_files.push(result);
                }
            }
        }

        // Sort file results by score descending
        exact_files.sort_by(|a, b| b.score.cmp(&a.score));
        other_files.sort_by(|a, b| b.score.cmp(&a.score));

        // 2. Grep search for non-empty queries
        let mut line_results = Vec::new();
        let mut _searchable_files = 0usize;
        let mut grep_result_opt = None;

        if scope != SearchScope::FileOnly && !highlight_query.is_empty() {
            let grep_mode = if mode.fuzzy {
                GrepMode::Fuzzy
            } else if mode.regex {
                GrepMode::Regex
            } else {
                GrepMode::PlainText
            };

            let grep_options = GrepSearchOptions {
                mode: grep_mode,
                smart_case: !mode.case_sensitive,
                time_budget_ms,
                file_offset: 0,
                page_limit: limit,
                classify_definitions: true,
                ..Default::default()
            };

            let grep_result = picker.grep(&parsed, &grep_options);
            _searchable_files = grep_result.filtered_file_count;
            total_matched += grep_result.matches.len();

            for m in &grep_result.matches {
                if m.file_index >= grep_result.files.len() {
                    continue;
                }
                let file = grep_result.files[m.file_index];
                let relative_path = file.relative_path(picker);
                let absolute_path = file.absolute_path(picker, &self.base_path);
                let column = m.match_byte_offsets.first().map(|(start, _)| *start + 1);
                line_results.push(UnifiedResult {
                    kind: MatchKind::Line,
                    relative_path,
                    absolute_path: absolute_path.to_string_lossy().into_owned(),
                    score: 0,
                    exact_match: false,
                    line_number: Some(m.line_number),
                    column,
                    line_content: Some(m.line_content.clone()),
                    match_byte_offsets: Some(
                        m.match_byte_offsets.iter().map(|&(a, b)| (a, b)).collect(),
                    ),
                    is_definition: Some(m.is_definition),
                    git_status: format_git_status_opt(file.git_status).map(|s| s.to_string()),
                });
            }

            grep_result_opt = Some(grep_result);
        }

        // 3. Assemble final results
        // In unified mode, suppress weak fuzzy path matches when there are
        // content results. The fuzzy matcher is very permissive (max_typos
        // scales with query length), so short queries like "struct" can
        // match almost every file path. Without a cutoff, dozens of
        // unrelated files get appended after grep results.
        if scope == SearchScope::Unified && !line_results.is_empty() {
            let min_score = highlight_query.len().saturating_mul(7) as i32;
            other_files.retain(|f| f.score >= min_score);
        }

        let mut unified = Vec::with_capacity(exact_files.len() + line_results.len() + other_files.len());
        unified.extend(exact_files.clone());

        if mode.group_grep && !line_results.is_empty() {
            let mut grouped = Vec::new();
            let mut last_path: Option<String> = None;
            for r in &line_results {
                if last_path.as_ref() != Some(&r.relative_path) {
                    grouped.push(UnifiedResult {
                        kind: MatchKind::FileHeader,
                        relative_path: r.relative_path.clone(),
                        absolute_path: r.absolute_path.clone(),
                        score: 0,
                        exact_match: false,
                        git_status: r.git_status.clone(),
                        ..Default::default()
                    });
                    last_path = Some(r.relative_path.clone());
                }
                grouped.push(r.clone());
            }
            unified.extend(grouped);
        } else {
            unified.extend(line_results.clone());
        }

        unified.extend(other_files.clone());

        SearchOutput {
            results: unified,
            highlight_query,
            fuzzy_total_matched: total_matched.saturating_sub(grep_result_opt.as_ref().map(|g| g.matches.len()).unwrap_or(0)),
            grep_page_matched: grep_result_opt.as_ref().map(|g| g.matches.len()).unwrap_or(0),
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_picker_backend_new() {
        let backend = PickerBackend::new(".").unwrap();
        // Wait a bit for scan to start
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(!backend.is_scanning() || backend.total_files() > 0);
    }

    #[test]
    fn test_search_empty_query() {
        let backend = PickerBackend::new(".").unwrap();
        // Wait for scan
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let output = backend.search("", SearchMode::default(), SearchScope::default(), None, false, usize::MAX, 0);
        assert!(output.fuzzy_total_matched > 0 || backend.is_scanning());
    }

    #[test]
    fn test_search_with_query() {
        let backend = PickerBackend::new(".").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let output = backend.search("main", SearchMode::default(), SearchScope::default(), None, false, usize::MAX, 0);
        // Should find src/main.rs or similar
        let has_main = output.results.iter().any(|r| {
            r.relative_path.contains("main")
                || r.line_content.as_ref().is_some_and(|c| c.contains("main"))
        });
        assert!(has_main || output.fuzzy_total_matched == 0);
    }

    #[test]
    fn test_unified_search_filters_weak_fuzzy_matches() {
        // Use the parent monorepo so there are enough files to produce weak
        // fuzzy matches for "struct".
        let backend = PickerBackend::new("../fff.nvim").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3000));

        let unified = backend.search(
            "struct",
            SearchMode::default(),
            SearchScope::Unified,
            None,
            false,
            usize::MAX,
            0,
        );

        // There should be grep results, otherwise the test premise is wrong.
        assert!(
            unified.grep_page_matched > 0,
            "expected grep matches for 'struct'"
        );

        // fuzzy_total_matched must still report the *true* total from the
        // backend (before filtering).
        assert!(
            unified.fuzzy_total_matched >= 10,
            "expected many fuzzy path matches for 'struct'"
        );

        // Weak matches like git.rs (score 35) should be filtered out in
        // unified mode because the min_score threshold for a 6-char query
        // is 42.
        let has_git_rs = unified.results.iter().any(|r| {
            r.kind == MatchKind::File && r.relative_path.ends_with("git.rs")
        });
        assert!(
            !has_git_rs,
            "git.rs should be filtered out in unified mode (weak fuzzy match)"
        );

        // In FileOnly mode the same weak match should still appear.
        let file_only = backend.search(
            "struct",
            SearchMode::default(),
            SearchScope::FileOnly,
            None,
            false,
            usize::MAX,
            0,
        );
        let has_git_rs_file_only = file_only.results.iter().any(|r| {
            r.kind == MatchKind::File && r.relative_path.ends_with("git.rs")
        });
        assert!(
            has_git_rs_file_only,
            "git.rs should appear in FileOnly mode"
        );
    }
}
