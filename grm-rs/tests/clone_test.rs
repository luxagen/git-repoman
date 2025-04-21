use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

// Integration test for GRM's clone mode
#[test]
fn test_grm_clone() -> Result<(), Box<dyn std::error::Error>> {
    // Setup test environment
    let test_dir = setup_test_environment()?;
    
    // Create upstream repository
    create_upstream_repository(&test_dir)?;
    
    // Run grm clone command
    let output = run_grm_clone(&test_dir)?;
    
    // Verify results
    verify_clone_results(&test_dir, &output)?;
    
    // Cleanup (optional - uncomment if you want tests to clean up after themselves)
    // cleanup(&test_dir)?;
    
    Ok(())
}

fn setup_test_environment() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Create base test directory
    let current_dir = env::current_dir()?;
    let timestamp = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let test_dir = current_dir.join(format!("test_grm_clone_{}", timestamp));
    fs::create_dir_all(&test_dir)?;
    
    // Create .grm.conf file
    let home_dir = env::var("HOME").or_else(|_| env::var("USERPROFILE"))?;
    let upstream_dir = Path::new(&home_dir).join(".grm_test_upstream");
    
    let config_content = format!(
        "ROOT={}\n\
         REPOS={}\n\
         RLOGIN=ssh://localhost\n\
         RPATH_ROOT={}\n\
         RPATH_TEMPLATE={}/template.git\n",
        test_dir.display(),
        test_dir.join(".grm.repos").display(),
        upstream_dir.display(),
        upstream_dir.display()
    );
    
    let conf_path = test_dir.join(".grm.conf");
    let mut conf_file = File::create(&conf_path)?;
    conf_file.write_all(config_content.as_bytes())?;
    
    // Create .grm.repos file with a single test repository
    let repos_content = "test/repo\ttest/repo\t.\n";
    let repos_path = test_dir.join(".grm.repos");
    let mut repos_file = File::create(&repos_path)?;
    repos_file.write_all(repos_content.as_bytes())?;
    
    Ok(test_dir)
}

fn create_upstream_repository(test_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Parse config to find upstream location
    let conf_path = test_dir.join(".grm.conf");
    let config_content = fs::read_to_string(&conf_path)?;
    
    // Extract RPATH_ROOT from config
    let rpath_root = config_content
        .lines()
        .find_map(|line| {
            if line.starts_with("RPATH_ROOT=") {
                Some(line.trim_start_matches("RPATH_ROOT="))
            } else {
                None
            }
        })
        .ok_or("RPATH_ROOT not found in config")?;
    
    // Create the upstream directory
    fs::create_dir_all(rpath_root)?;
    
    // Create a bare repository for "test/repo"
    let repo_path = Path::new(rpath_root).join("test/repo.git");
    fs::create_dir_all(repo_path.parent().unwrap())?;
    
    // Run git init --bare
    let output = Command::new("git")
        .args(["init", "--bare", "--quiet"])
        .current_dir(&repo_path)
        .output()?;
    
    if !output.status.success() {
        return Err(format!(
            "Failed to create bare repository: {}",
            String::from_utf8_lossy(&output.stderr)
        ).into());
    }
    
    Ok(())
}

fn run_grm_clone(test_dir: &Path) -> Result<Output, Box<dyn std::error::Error>> {
    // Get path to grm-rs executable
    let exec_path = env::current_dir()?.join("target/debug/grm-rs");
    
    // Run grm-rs clone
    let output = Command::new(exec_path)
        .args(["clone"])
        .current_dir(test_dir)
        .output()?;
    
    println!("GRM clone output: {}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        println!("GRM clone error: {}", String::from_utf8_lossy(&output.stderr));
    }
    
    Ok(output)
}

fn verify_clone_results(test_dir: &Path, output: &Output) -> Result<(), Box<dyn std::error::Error>> {
    // Check if the command succeeded
    if !output.status.success() {
        return Err(format!(
            "GRM clone failed with status: {:?}",
            output.status
        ).into());
    }
    
    // Check if the repository was cloned properly
    let repo_path = test_dir.join("test/repo");
    
    // Verify directory exists
    if !repo_path.exists() {
        return Err(format!("Repository was not cloned to {}", repo_path.display()).into());
    }
    
    // Verify it's a git repository
    let git_check = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(&repo_path)
        .output()?;
    
    if !git_check.status.success() {
        return Err(format!("Cloned directory is not a git repository: {}", repo_path.display()).into());
    }
    
    // Verify it has the correct remote
    let remote_check = Command::new("git")
        .args(["remote", "-v"])
        .current_dir(&repo_path)
        .output()?;
    
    let remote_output = String::from_utf8_lossy(&remote_check.stdout);
    if !remote_output.contains("origin") {
        return Err("Remote 'origin' not found in cloned repository".into());
    }
    
    Ok(())
}

fn cleanup(test_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Clean up test directories
    fs::remove_dir_all(test_dir)?;
    
    // Clean up upstream repository
    let conf_path = test_dir.join(".grm.conf");
    let config_content = fs::read_to_string(&conf_path)?;
    
    // Extract RPATH_ROOT from config
    if let Some(rpath_root) = config_content
        .lines()
        .find_map(|line| {
            if line.starts_with("RPATH_ROOT=") {
                Some(line.trim_start_matches("RPATH_ROOT="))
            } else {
                None
            }
        }) 
    {
        let upstream_path = Path::new(rpath_root);
        if upstream_path.exists() {
            fs::remove_dir_all(upstream_path)?;
        }
    }
    
    Ok(())
}
