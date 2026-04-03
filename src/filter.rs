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
}
