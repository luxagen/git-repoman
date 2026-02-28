use anyhow::{Result, anyhow};
use gix_url::{Scheme, Url};
use bstr::BStr;

/// Normalize a path for use in URLs
/// 
/// This ensures special characters are properly encoded
fn normalize_path(path: &str) -> String {
    // Just unescape any backslash-escaped characters
    unescape_backslashes(path)
}

/// Unescape backslash sequences in a string
/// Simply removes backslashes, treating the character after each as a literal
fn unescape_backslashes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some() {
            // Skip the backslash and add whatever character follows it
            if let Some(next_char) = chars.next() {
                result.push(next_char);
            }
        } else {
            result.push(c);
        }
    }
    
    result
}

/// Parse and normalize a Git remote URL
/// 
/// Handles three types of URLs:
/// - Local paths
/// - HTTP(S) URLs
/// - SSH URLs
/// 
/// Returns the normalized URL that can be used with Git operations
pub fn parse_remote_url(url_str: &str) -> Result<String> {
    // Parse the URL using gix-url - convert str to BStr
    let parsed = gix_url::parse(url_str.as_bytes().into())
        .map_err(|e| anyhow!("Failed to parse remote URL: {}", e))?;

    // Return different formats based on the scheme
    match parsed.scheme {
        Scheme::File => {
            // Local file path
            Ok(parsed.path.to_string())
        },
        Scheme::Https | Scheme::Http => {
            // HTTP(S) URL - return as is but normalized
            Ok(parsed.to_string())
        },
        Scheme::Ssh => {
            // SSH URL - properly format as user@host:path
            // Use the to_string() method which handles the proper formatting
            Ok(parsed.to_string())
        },
        _ => {
            // Other schemes - return as is
            Ok(parsed.to_string())
        }
    }
}

/// Build a Git clone/fetch URL from components
/// 
/// * `rlogin` - Optional remote login info (e.g., "user@host" or "https://github.com")
/// * `remote_dir` - Remote directory path
/// * `repo_path` - Repository path
pub fn build_remote_url(rlogin: Option<&str>, remote_dir: &str, repo_path: &str) -> String {
    match rlogin {
        Some(login) if !login.is_empty() => {
            let login = login.trim_end_matches('/');
            
            if login.contains("://") {
                // Protocol-based URL (http://, https://, ssh://)
                let login_parts: Vec<&str> = login.splitn(2, "://").collect();
                let protocol = login_parts[0];
                let domain = login_parts[1].trim_end_matches('/');
                
                // For HTTP/HTTPS, use URL encoding through gix-url
                if protocol == "http" || protocol == "https" {
                    // Create a full URL with the path
                    let path = format!("{}/{}", 
                        remote_dir.trim_matches('/'),
                        repo_path.trim_start_matches('/'));
                    
                    let full_url = format!("{}://{}/{}", protocol, domain.trim_end_matches('/'), path);
                    
                    // Try to parse and normalize with gix-url
                    if let Ok(parsed) = gix_url::parse(full_url.as_bytes().into()) {
                        return parsed.to_string();
                    }
                    
                    // Fall back to simple string formatting if parsing fails
                    return full_url;
                } else if protocol == "ssh" {
                    // SSH protocol with explicit scheme
                    // For SSH, spaces should be preserved, not URL-encoded
                    let path = format!("{}/{}", 
                        unescape_backslashes(remote_dir).trim_matches('/'),
                        unescape_backslashes(repo_path).trim_start_matches('/'));
                    
                    return format!("{}://{}/{}", protocol, domain, path);
                } else {
                    // Other protocols, handle generically
                    let path = format!("{}/{}", 
                        unescape_backslashes(remote_dir).trim_matches('/'),
                        unescape_backslashes(repo_path).trim_start_matches('/'));
                    
                    return format!("{}://{}/{}", protocol, domain, path);
                }
            } else {
                // SSH SCP-style syntax (user@host:path)
                // For SSH, spaces and special chars should be preserved, not URL-encoded
                let path = format!("{}/{}", 
                    unescape_backslashes(remote_dir).trim_matches('/'),
                    unescape_backslashes(repo_path).trim_start_matches('/'));
                
                format!("{}:{}", login, path)
            }
        },
        _ => {
            // Local path - just unescape and join
            format!("{}/{}", 
                unescape_backslashes(remote_dir).trim_end_matches('/'),
                unescape_backslashes(repo_path).trim_start_matches('/'))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_local_path() {
        let result = parse_remote_url("/path/to/repo.git").unwrap();
        assert_eq!(result, "/path/to/repo.git");
    }

    #[test]
    fn test_parse_https_url() {
        let result = parse_remote_url("https://github.com/user/repo.git").unwrap();
        assert_eq!(result, "https://github.com/user/repo.git");
    }

    #[test]
    fn test_parse_ssh_url() {
        let result = parse_remote_url("ssh://user@github.com/user/repo.git").unwrap();
        assert_eq!(result, "ssh://user@github.com/user/repo.git");
    }

    #[test]
    fn test_build_remote_url_with_login() {
        let result = build_remote_url(
            Some("user@github.com"), 
            "organization", 
            "repository.git"
        );
        assert_eq!(result, "user@github.com:organization/repository.git");
    }

    #[test]
    fn test_build_remote_url_without_login() {
        let result = build_remote_url(
            None, 
            "organization", 
            "repository.git"
        );
        assert_eq!(result, "organization/repository.git");
    }

    #[test]
    fn test_build_remote_url_with_protocol() {
        let result = build_remote_url(
            Some("https://github.com"), 
            "organization", 
            "repository.git"
        );
        assert_eq!(result, "https://github.com/organization/repository.git");
    }

    #[test]
    fn test_normalize_path_with_special_chars() {
        assert_eq!(normalize_path("path/with spaces/[brackets]"), "path/with spaces/[brackets]");
    }

    #[test]
    fn test_normalize_path_with_escaped_chars() {
        assert_eq!(normalize_path("path/with\\[escaped\\]brackets"), "path/with[escaped]brackets");
    }
}
