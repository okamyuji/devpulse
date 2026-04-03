use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use regex::Regex;

enum MatchMode {
    None,
    Fuzzy(String),
    Regex(Regex),
}

pub struct FilterState {
    query: String,
    mode: MatchMode,
    matcher: SkimMatcherV2,
}

impl std::fmt::Debug for FilterState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilterState")
            .field("query", &self.query)
            .finish()
    }
}

impl Default for FilterState {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            mode: MatchMode::None,
            matcher: SkimMatcherV2::default(),
        }
    }

    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        if let Some(pattern) = query.strip_prefix("regex:") {
            match Regex::new(pattern) {
                Ok(re) => self.mode = MatchMode::Regex(re),
                Err(_) => self.mode = MatchMode::Fuzzy(query.to_string()),
            }
        } else if query.is_empty() {
            self.mode = MatchMode::None;
        } else {
            self.mode = MatchMode::Fuzzy(query.to_string());
        }
    }

    pub fn matches(&self, text: &str) -> bool {
        match &self.mode {
            MatchMode::None => true,
            MatchMode::Fuzzy(q) => self.matcher.fuzzy_match(text, q).is_some(),
            MatchMode::Regex(re) => re.is_match(text),
        }
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.mode = MatchMode::None;
    }

    pub fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// AND条件マッチング: クエリを半角/全角スペースで分割し、
    /// 全トークンがテキスト(source+message連結)にマッチする場合のみtrue。
    /// クエリが空の場合は常にtrue。
    pub fn matches_all_terms(&self, text: &str) -> bool {
        if self.query.is_empty() {
            return true;
        }
        // Split on half-width and full-width spaces
        let terms: Vec<&str> = self
            .query
            .split([' ', '\u{3000}'])
            .filter(|s| !s.is_empty())
            .collect();
        if terms.is_empty() {
            return true;
        }
        let text_lower = text.to_lowercase();
        terms
            .iter()
            .all(|term| text_lower.contains(&term.to_lowercase()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_all_terms_empty_query() {
        let f = FilterState::new();
        assert!(f.matches_all_terms("anything"));
    }

    #[test]
    fn test_matches_all_terms_single_term() {
        let mut f = FilterState::new();
        f.set_query("error");
        assert!(f.matches_all_terms("[app-web] connection error occurred"));
        assert!(!f.matches_all_terms("[app-web] request ok"));
    }

    #[test]
    fn test_matches_all_terms_and_condition_halfwidth() {
        let mut f = FilterState::new();
        f.set_query("app error");
        assert!(f.matches_all_terms("[app-web] connection error"));
        assert!(!f.matches_all_terms("[db] connection error"));
        assert!(!f.matches_all_terms("[app-web] request ok"));
    }

    #[test]
    fn test_matches_all_terms_and_condition_fullwidth() {
        let mut f = FilterState::new();
        f.set_query("app\u{3000}error");
        assert!(f.matches_all_terms("[app-web] connection error"));
        assert!(!f.matches_all_terms("[db] connection error"));
    }

    #[test]
    fn test_matches_all_terms_case_insensitive() {
        let mut f = FilterState::new();
        f.set_query("ERROR App");
        assert!(f.matches_all_terms("[app-web] connection Error occurred"));
    }

    #[test]
    fn test_matches_all_terms_only_spaces() {
        let mut f = FilterState::new();
        f.set_query("   ");
        assert!(f.matches_all_terms("anything"));
    }

    #[test]
    fn test_matches_all_terms_mixed_spaces() {
        let mut f = FilterState::new();
        f.set_query("web\u{3000}timeout error");
        // All 3 terms must match
        assert!(f.matches_all_terms("[app-web] timeout error on connection"));
        assert!(!f.matches_all_terms("[app-web] timeout on connection"));
    }
}
