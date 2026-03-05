use thiserror::Error;

const MAX_SLUG_LENGTH: usize = 64;

#[derive(Debug, Error, PartialEq)]
pub enum SlugError {
    #[error("slug cannot be empty")]
    Empty,
    #[error("slug contains path separator")]
    PathSeparator,
    #[error("slug exceeds maximum length of {MAX_SLUG_LENGTH} characters")]
    TooLong,
}

/// Sanitize an input string into a URL/filesystem-safe slug.
pub fn slugify(input: &str) -> String {
    let lowered = input.to_lowercase();

    let replaced: String = lowered
        .chars()
        .map(|c| match c {
            ' ' | '_' => '-',
            c if c.is_ascii_alphanumeric() || c == '-' => c,
            _ => '\0',
        })
        .filter(|&c| c != '\0')
        .collect();

    // Collapse consecutive hyphens
    let mut result = String::with_capacity(replaced.len());
    let mut prev_hyphen = false;
    for c in replaced.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    result.trim_matches('-').to_string()
}

/// Validate that a slug meets all constraints.
pub fn validate_slug(slug: &str) -> Result<(), SlugError> {
    if slug.is_empty() || slugify(slug).is_empty() {
        return Err(SlugError::Empty);
    }
    if slug.contains('/') || slug.contains('\\') {
        return Err(SlugError::PathSeparator);
    }
    if slug.len() > MAX_SLUG_LENGTH {
        return Err(SlugError::TooLong);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_slugification() {
        assert_eq!(slugify("My Feature"), "my-feature");
    }

    #[test]
    fn spaces_and_underscores() {
        assert_eq!(slugify("foo_bar baz"), "foo-bar-baz");
    }

    #[test]
    fn special_characters_stripped() {
        assert_eq!(slugify("hello@world!"), "helloworld");
    }

    #[test]
    fn consecutive_hyphens_collapsed() {
        assert_eq!(slugify("foo--bar"), "foo-bar");
    }

    #[test]
    fn leading_trailing_hyphens_trimmed() {
        assert_eq!(slugify("-foo-"), "foo");
    }

    #[test]
    fn empty_input_validation_fails() {
        assert_eq!(validate_slug(""), Err(SlugError::Empty));
    }

    #[test]
    fn only_special_chars_validation_fails() {
        assert_eq!(validate_slug("@#$%"), Err(SlugError::Empty));
    }

    #[test]
    fn path_separator_forward_slash_fails() {
        assert_eq!(validate_slug("foo/bar"), Err(SlugError::PathSeparator));
    }

    #[test]
    fn path_separator_backslash_fails() {
        assert_eq!(validate_slug("foo\\bar"), Err(SlugError::PathSeparator));
    }

    #[test]
    fn long_name_validation_fails() {
        let long = "a".repeat(65);
        assert_eq!(validate_slug(&long), Err(SlugError::TooLong));
    }

    #[test]
    fn valid_slug_passes() {
        assert!(validate_slug("my-feature").is_ok());
    }

    #[test]
    fn max_length_slug_passes() {
        let exactly_64 = "a".repeat(64);
        assert!(validate_slug(&exactly_64).is_ok());
    }

    #[test]
    fn mixed_case_lowered() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn multiple_transformations_combined() {
        assert_eq!(slugify("  __My Cool--Feature!__ "), "my-cool-feature");
    }
}
