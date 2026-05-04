use std::collections::HashSet;

/// A parsed group filter expression (e.g. `"default,-vendor,tools"`).
///
/// Groups prefixed with `-` are exclusions; all others are inclusions.
/// If no positive groups are specified, `"default"` is implied.
#[derive(Debug, Clone)]
pub struct GroupFilter {
    pub include: HashSet<String>,
    pub exclude: HashSet<String>,
}

impl GroupFilter {
    /// Parse a comma-separated groups expression.
    ///
    /// ```text
    /// "default"          → include={default}, exclude={}
    /// "default,-vendor"  → include={default}, exclude={vendor}
    /// "-vendor"          → include={default}, exclude={vendor}  (implicit default)
    /// "all"              → include={all},     exclude={}
    /// ""                 → include={default}, exclude={}
    /// ```
    pub fn parse(input: &str) -> Self {
        let mut include = HashSet::new();
        let mut exclude = HashSet::new();

        for token in input.split([',', ' ']) {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            if let Some(stripped) = token.strip_prefix('-') {
                if !stripped.is_empty() {
                    exclude.insert(stripped.to_string());
                }
            } else {
                include.insert(token.to_string());
            }
        }

        // If no positive groups were specified, default to "default".
        if include.is_empty() {
            include.insert("default".to_string());
        }

        GroupFilter { include, exclude }
    }

    /// Check whether a project with the given explicit groups matches this filter.
    ///
    /// Implicit groups are applied automatically:
    /// - Every project belongs to `"all"`.
    /// - Every project belongs to `"default"` unless its groups contain `"notdefault"`.
    pub fn matches(&self, project_groups: &[String]) -> bool {
        let mut effective: HashSet<&str> =
            project_groups.iter().map(|s| s.as_str()).collect();
        effective.insert("all");
        if !effective.contains("notdefault") {
            effective.insert("default");
        }

        let dominated = self.include.iter().any(|g| effective.contains(g.as_str()));
        let excluded = self.exclude.iter().any(|g| effective.contains(g.as_str()));
        dominated && !excluded
    }
}

impl Default for GroupFilter {
    fn default() -> Self {
        Self::parse("default")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_group() {
        let f = GroupFilter::parse("default");
        assert!(f.include.contains("default"));
        assert!(f.exclude.is_empty());
    }

    #[test]
    fn parse_empty_string_implies_default() {
        let f = GroupFilter::parse("");
        assert!(f.include.contains("default"));
    }

    #[test]
    fn parse_exclusion_only_implies_default() {
        let f = GroupFilter::parse("-vendor");
        assert!(f.include.contains("default"));
        assert!(f.exclude.contains("vendor"));
    }

    #[test]
    fn parse_mixed_groups() {
        let f = GroupFilter::parse("default,-vendor,tools");
        assert!(f.include.contains("default"));
        assert!(f.include.contains("tools"));
        assert!(f.exclude.contains("vendor"));
        assert_eq!(f.include.len(), 2);
        assert_eq!(f.exclude.len(), 1);
    }

    #[test]
    fn matches_project_with_no_groups_against_default() {
        let f = GroupFilter::parse("default");
        assert!(f.matches(&[]));
    }

    #[test]
    fn matches_all_includes_every_project() {
        let f = GroupFilter::parse("all");
        assert!(f.matches(&[]));
        assert!(f.matches(&["vendor".into()]));
        assert!(f.matches(&["notdefault".into()]));
    }

    #[test]
    fn matches_all_minus_vendor_excludes_vendor() {
        let f = GroupFilter::parse("all,-vendor");
        assert!(f.matches(&[]));
        assert!(!f.matches(&["vendor".into()]));
    }

    #[test]
    fn matches_default_minus_vendor_excludes_vendor() {
        let f = GroupFilter::parse("default,-vendor");
        assert!(f.matches(&[])); // implicit default
        assert!(!f.matches(&["vendor".into()]));
    }

    #[test]
    fn matches_notdefault_project_excluded_from_default_filter() {
        let f = GroupFilter::parse("default");
        assert!(!f.matches(&["notdefault".into()]));
    }

    #[test]
    fn matches_notdefault_project_included_by_explicit_group() {
        let f = GroupFilter::parse("vendor");
        assert!(f.matches(&["notdefault".into(), "vendor".into()]));
    }

    #[test]
    fn matches_specific_group_only_matches_members() {
        let f = GroupFilter::parse("vendor");
        assert!(f.matches(&["vendor".into()]));
        assert!(!f.matches(&["tools".into()]));
    }

    #[test]
    fn matches_exclusion_only_filter_uses_implicit_default() {
        let f = GroupFilter::parse("-vendor");
        // Project without groups → in "default" → matches
        assert!(f.matches(&[]));
        // Project in "vendor" → in "default" but excluded by vendor
        assert!(!f.matches(&["vendor".into()]));
    }

    #[test]
    fn parse_whitespace_and_duplicates_handled() {
        let f = GroupFilter::parse(" default , tools , -vendor , tools ");
        assert_eq!(f.include.len(), 2); // default, tools (deduplicated)
        assert_eq!(f.exclude.len(), 1);
    }

    #[test]
    fn matches_order_independence() {
        // "all,-vendor" and "-vendor,all" should behave identically
        let f1 = GroupFilter::parse("all,-vendor");
        let f2 = GroupFilter::parse("-vendor,all");

        let cases: Vec<Vec<String>> = vec![
            vec![],
            vec!["vendor".into()],
            vec!["tools".into()],
            vec!["notdefault".into()],
        ];

        for groups in &cases {
            assert_eq!(
                f1.matches(groups),
                f2.matches(groups),
                "mismatch for groups={groups:?}"
            );
        }
    }
}
