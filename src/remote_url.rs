// GRM - Git Repository Manager
// Copyright © luxagen, 2025-present

use anyhow::{Result, anyhow};
use gix_url::{Scheme, Url};
use bstr::BStr;

/// Build a Git clone/fetch URL from components
/// 
/// * `rlogin` - Optional remote login info (e.g., "user@host" or "https://github.com")
/// * `remote_dir` - Remote directory path
/// * `repo_path` - Repository path
pub fn build_remote_url(rlogin: &str, remote_dir: &str, repo_path: &str) -> String {
    if rlogin.is_empty() {
        // Local path - just join
        return format!("{}/{}", 
            remote_dir.trim_end_matches('/'),
            repo_path.trim_start_matches('/'));
    }

    let login = rlogin.trim_end_matches('/');
    if login.contains("://") {
        // Protocol-based URL (http://, https://, ssh://, etc)
        let login_parts: Vec<&str> = login.splitn(2, "://").collect();
        let protocol = login_parts[0];
        let domain = login_parts[1].trim_end_matches('/');
        let path = format!("{}/{}", remote_dir.trim_matches('/'), repo_path.trim_start_matches('/'));
        match protocol {
            "http" | "https" => {
                let full_url = format!("{}://{}/{}", protocol, domain.trim_end_matches('/'), path);
                // Try to parse and normalize with gix-url
                if let Ok(parsed) = gix_url::parse(full_url.as_bytes().into()) {
                    return parsed.to_string();
                }
                // Fall back to simple string formatting if parsing fails
                full_url
            },
            _ => format!("{}://{}/{}", protocol, domain, path)
        }
    } else {
        format!("{}:{}/{}", login, remote_dir.trim_matches('/'), repo_path.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_remote_url_with_login() {
        let result = build_remote_url(
            "user@github.com",
            "organization",
            "repository.git"
        );
        assert_eq!(result, "user@github.com:organization/repository.git");
    }

    #[test]
    fn test_build_remote_url_without_login() {
        let result = build_remote_url(
            "",
            "organization",
            "repository.git"
        );
        assert_eq!(result, "organization/repository.git");
    }

    #[test]
    fn test_build_remote_url_with_protocol() {
        let result = build_remote_url(
            "https://github.com",
            "organization",
            "repository.git"
        );
        assert_eq!(result, "https://github.com/organization/repository.git");
    }


}
