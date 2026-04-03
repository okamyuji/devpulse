use devpulse::filter::FilterState;

#[test]
fn test_empty_filter_matches_all() {
    let filter = FilterState::new();
    assert!(filter.matches("anything"));
    assert!(filter.matches(""));
}

#[test]
fn test_fuzzy_filter() {
    let mut filter = FilterState::new();
    filter.set_query("nde");
    assert!(filter.matches("next-dev"));
    assert!(filter.matches("node"));
    assert!(!filter.matches("postgres"));
}

#[test]
fn test_regex_filter() {
    let mut filter = FilterState::new();
    filter.set_query("regex:^node.*dev$");
    assert!(filter.matches("node-dev"));
    assert!(filter.matches("nodejs-dev"));
    assert!(!filter.matches("node"));
    assert!(!filter.matches("dev-node"));
}

#[test]
fn test_invalid_regex_falls_back_to_fuzzy() {
    let mut filter = FilterState::new();
    filter.set_query("regex:[invalid");
    assert!(filter.matches("regex:[invalid"));
}

#[test]
fn test_clear_filter() {
    let mut filter = FilterState::new();
    filter.set_query("node");
    assert!(!filter.matches("postgres"));
    filter.clear();
    assert!(filter.matches("postgres"));
}

#[test]
fn test_is_active() {
    let mut filter = FilterState::new();
    assert!(!filter.is_active());
    filter.set_query("test");
    assert!(filter.is_active());
    filter.clear();
    assert!(!filter.is_active());
}
