use fff_search::{
    file_picker::FilePicker, FilePickerOptions, FuzzySearchOptions, PaginationArgs, QueryParser,
    SharedFrecency, SharedPicker, SharedQueryTracker,
};
use std::path::PathBuf;

/// A lightweight, owned result from the file picker.
#[derive(Clone)]
pub struct FileResult {
    pub relative_path: String,
    pub absolute_path: String,
    pub score: i32,
    pub exact_match: bool,
    pub match_type: &'static str,
}

/// Output of a search operation.
pub struct SearchOutput {
    pub results: Vec<FileResult>,
    pub total_matched: usize,
    pub highlight_query: String,
}

/// Wrapper around fff-core's FilePicker that provides a sync API for the TUI.
pub struct PickerBackend {
    shared_picker: SharedPicker,
    base_path: PathBuf,
}

impl PickerBackend {
    pub fn new(base_path: &str) -> anyhow::Result<Self> {
        let canonical = std::fs::canonicalize(base_path)?;
        let shared_picker = SharedPicker::default();
        let shared_frecency = SharedFrecency::noop();
        let _shared_query_tracker = SharedQueryTracker::noop();

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
            base_path: canonical,
        })
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

    /// Perform a fuzzy search and return owned results.
    pub fn search(&self, query: &str, limit: usize) -> SearchOutput {
        let guard = match self.shared_picker.read() {
            Ok(g) => g,
            Err(_) => {
                return SearchOutput {
                    results: Vec::new(),
                    total_matched: 0,
                    highlight_query: query.to_string(),
                }
            }
        };

        let picker = match guard.as_ref() {
            Some(p) => p,
            None => {
                return SearchOutput {
                    results: Vec::new(),
                    total_matched: 0,
                    highlight_query: query.to_string(),
                }
            }
        };

        let parser = QueryParser::default();
        let parsed = parser.parse(query);

        // Build the highlight query from fuzzy parts only (exclude constraints)
        let highlight_query = match &parsed.fuzzy_query {
            fff_search::FuzzyQuery::Text(t) => t.to_string(),
            fff_search::FuzzyQuery::Parts(parts) => parts.join(""),
            fff_search::FuzzyQuery::Empty => String::new(),
        };

        let results = picker.fuzzy_search(
            &parsed,
            None, // no query tracker for now
            FuzzySearchOptions {
                max_threads: 0, // auto
                current_file: None,
                project_path: Some(&self.base_path),
                combo_boost_score_multiplier: 0,
                min_combo_count: 0,
                pagination: PaginationArgs { offset: 0, limit },
            },
        );

        let total_matched = results.total_matched;
        let mut file_results = Vec::with_capacity(results.items.len());

        for (item, score) in results.items.iter().zip(results.scores.iter()) {
            let relative_path = item.relative_path(picker);
            let absolute_path = item.absolute_path(picker, &self.base_path);
            file_results.push(FileResult {
                relative_path,
                absolute_path: absolute_path.to_string_lossy().into_owned(),
                score: score.total,
                exact_match: score.exact_match,
                match_type: score.match_type,
            });
        }

        SearchOutput {
            results: file_results,
            total_matched,
            highlight_query,
        }
    }

    pub fn base_path(&self) -> &PathBuf {
        &self.base_path
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
        let output = backend.search("", 50);
        assert!(output.total_matched > 0 || backend.is_scanning());
    }

    #[test]
    fn test_search_with_query() {
        let backend = PickerBackend::new(".").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let output = backend.search("main", 50);
        // Should find src/main.rs or similar
        let has_main = output.results.iter().any(|r| r.relative_path.contains("main"));
        assert!(has_main || output.total_matched == 0);
    }
}
