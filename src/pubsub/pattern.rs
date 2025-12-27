//! Glob pattern matching for PSUBSCRIBE
//!
//! Implements Redis-style glob pattern matching for pub/sub pattern subscriptions.
//! Supports:
//! - `*` matches any sequence of characters (including empty)
//! - `?` matches exactly one character
//! - `[abc]` matches one character from the set
//! - `[^abc]` or `[!abc]` matches one character NOT in the set
//! - `[a-z]` matches one character in the range
//! - `\` escapes special characters

/// Pattern matcher for glob-style patterns
#[derive(Debug, Clone)]
pub struct PatternMatcher {
    pattern: String,
}

impl PatternMatcher {
    /// Create a new pattern matcher from a pattern string
    pub fn new(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
        }
    }

    /// Check if a string matches this pattern
    pub fn matches(&self, text: &str) -> bool {
        Self::match_pattern(&self.pattern, text)
    }

    /// Get the pattern string
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Internal pattern matching implementation
    fn match_pattern(pattern: &str, text: &str) -> bool {
        let pattern_chars: Vec<char> = pattern.chars().collect();
        let text_chars: Vec<char> = text.chars().collect();

        Self::match_recursive(&pattern_chars, 0, &text_chars, 0)
    }

    fn match_recursive(
        pattern: &[char],
        mut pi: usize,
        text: &[char],
        mut ti: usize,
    ) -> bool {
        while pi < pattern.len() {
            match pattern[pi] {
                '*' => {
                    // Skip consecutive stars
                    while pi < pattern.len() && pattern[pi] == '*' {
                        pi += 1;
                    }

                    // If star is at end, it matches everything remaining
                    if pi >= pattern.len() {
                        return true;
                    }

                    // Try matching star with 0, 1, 2, ... characters
                    for i in ti..=text.len() {
                        if Self::match_recursive(pattern, pi, text, i) {
                            return true;
                        }
                    }
                    return false;
                }
                '?' => {
                    // Must match exactly one character
                    if ti >= text.len() {
                        return false;
                    }
                    pi += 1;
                    ti += 1;
                }
                '[' => {
                    // Character class
                    if ti >= text.len() {
                        return false;
                    }

                    let (matched, new_pi) = Self::match_char_class(pattern, pi, text[ti]);
                    if !matched {
                        return false;
                    }
                    pi = new_pi;
                    ti += 1;
                }
                '\\' => {
                    // Escaped character
                    pi += 1;
                    if pi >= pattern.len() {
                        return false;
                    }
                    if ti >= text.len() || pattern[pi] != text[ti] {
                        return false;
                    }
                    pi += 1;
                    ti += 1;
                }
                c => {
                    // Literal character match
                    if ti >= text.len() || c != text[ti] {
                        return false;
                    }
                    pi += 1;
                    ti += 1;
                }
            }
        }

        // Pattern consumed, text must also be consumed
        ti >= text.len()
    }

    /// Match a character class [...]
    /// Returns (matched, new_pattern_index)
    fn match_char_class(pattern: &[char], start: usize, c: char) -> (bool, usize) {
        let mut pi = start + 1; // Skip '['
        let mut negated = false;
        let mut matched = false;

        // Check for negation
        if pi < pattern.len() && (pattern[pi] == '^' || pattern[pi] == '!') {
            negated = true;
            pi += 1;
        }

        // Handle ] as first character (literal ])
        if pi < pattern.len() && pattern[pi] == ']' {
            if c == ']' {
                matched = true;
            }
            pi += 1;
        }

        while pi < pattern.len() && pattern[pi] != ']' {
            if pi + 2 < pattern.len() && pattern[pi + 1] == '-' && pattern[pi + 2] != ']' {
                // Range: a-z
                let range_start = pattern[pi];
                let range_end = pattern[pi + 2];
                if c >= range_start && c <= range_end {
                    matched = true;
                }
                pi += 3;
            } else {
                // Single character
                if pattern[pi] == '\\' && pi + 1 < pattern.len() {
                    pi += 1; // Skip backslash
                }
                if pattern[pi] == c {
                    matched = true;
                }
                pi += 1;
            }
        }

        // Skip closing ]
        if pi < pattern.len() && pattern[pi] == ']' {
            pi += 1;
        }

        if negated {
            (!matched, pi)
        } else {
            (matched, pi)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let matcher = PatternMatcher::new("hello");
        assert!(matcher.matches("hello"));
        assert!(!matcher.matches("world"));
        assert!(!matcher.matches("hello!"));
        assert!(!matcher.matches("hell"));
    }

    #[test]
    fn test_star_wildcard() {
        let matcher = PatternMatcher::new("h*o");
        assert!(matcher.matches("hello"));
        assert!(matcher.matches("ho"));
        assert!(matcher.matches("hallo"));
        assert!(!matcher.matches("hello!"));

        let matcher = PatternMatcher::new("*");
        assert!(matcher.matches(""));
        assert!(matcher.matches("anything"));

        let matcher = PatternMatcher::new("h*");
        assert!(matcher.matches("h"));
        assert!(matcher.matches("hello"));
        assert!(!matcher.matches("world"));

        let matcher = PatternMatcher::new("*o");
        assert!(matcher.matches("o"));
        assert!(matcher.matches("hello"));
        assert!(!matcher.matches("world"));
    }

    #[test]
    fn test_question_wildcard() {
        let matcher = PatternMatcher::new("h?llo");
        assert!(matcher.matches("hello"));
        assert!(matcher.matches("hallo"));
        assert!(!matcher.matches("hllo"));
        assert!(!matcher.matches("heello"));

        let matcher = PatternMatcher::new("???");
        assert!(matcher.matches("abc"));
        assert!(!matcher.matches("ab"));
        assert!(!matcher.matches("abcd"));
    }

    #[test]
    fn test_char_class() {
        let matcher = PatternMatcher::new("h[ae]llo");
        assert!(matcher.matches("hello"));
        assert!(matcher.matches("hallo"));
        assert!(!matcher.matches("hillo"));

        let matcher = PatternMatcher::new("h[a-z]llo");
        assert!(matcher.matches("hello"));
        assert!(matcher.matches("hallo"));
        assert!(matcher.matches("hzllo"));
        assert!(!matcher.matches("h1llo"));
    }

    #[test]
    fn test_negated_char_class() {
        let matcher = PatternMatcher::new("h[^ae]llo");
        assert!(!matcher.matches("hello"));
        assert!(!matcher.matches("hallo"));
        assert!(matcher.matches("hillo"));
        assert!(matcher.matches("hullo"));

        let matcher = PatternMatcher::new("h[!0-9]llo");
        assert!(matcher.matches("hello"));
        assert!(!matcher.matches("h1llo"));
    }

    #[test]
    fn test_escaped_chars() {
        let matcher = PatternMatcher::new("h\\*llo");
        assert!(matcher.matches("h*llo"));
        assert!(!matcher.matches("hello"));

        let matcher = PatternMatcher::new("h\\?llo");
        assert!(matcher.matches("h?llo"));
        assert!(!matcher.matches("hello"));
    }

    #[test]
    fn test_redis_channel_patterns() {
        // Common Redis pub/sub patterns
        let matcher = PatternMatcher::new("news.*");
        assert!(matcher.matches("news.sports"));
        assert!(matcher.matches("news.tech"));
        assert!(matcher.matches("news."));
        assert!(!matcher.matches("news"));
        assert!(!matcher.matches("newsflash"));

        let matcher = PatternMatcher::new("user:*:events");
        assert!(matcher.matches("user:123:events"));
        assert!(matcher.matches("user:abc:events"));
        assert!(!matcher.matches("user:events"));
        assert!(!matcher.matches("user:123:messages"));

        let matcher = PatternMatcher::new("__keyspace@0__:*");
        assert!(matcher.matches("__keyspace@0__:mykey"));
        assert!(matcher.matches("__keyspace@0__:"));
        assert!(!matcher.matches("__keyspace@1__:mykey"));
    }

    #[test]
    fn test_complex_patterns() {
        let matcher = PatternMatcher::new("*[0-9]*");
        assert!(matcher.matches("abc123def"));
        assert!(matcher.matches("1"));
        assert!(!matcher.matches("abcdef"));

        let matcher = PatternMatcher::new("??[a-z]??");
        assert!(matcher.matches("01x23"));
        assert!(matcher.matches("aaxbb"));
        assert!(!matcher.matches("01123"));
    }

    #[test]
    fn test_empty_pattern_and_text() {
        let matcher = PatternMatcher::new("");
        assert!(matcher.matches(""));
        assert!(!matcher.matches("a"));

        let matcher = PatternMatcher::new("*");
        assert!(matcher.matches(""));
    }

    #[test]
    fn test_consecutive_stars() {
        let matcher = PatternMatcher::new("a**b");
        assert!(matcher.matches("ab"));
        assert!(matcher.matches("axb"));
        assert!(matcher.matches("axyb"));
    }

    #[test]
    fn test_pattern_getter() {
        let matcher = PatternMatcher::new("test.*");
        assert_eq!(matcher.pattern(), "test.*");
    }
}
