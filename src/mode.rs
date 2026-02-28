use clap::ValueEnum;
use once_cell::sync::OnceCell;

/// Operations that can be performed on repositories
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Operations {
    /// Clone the repository if it doesn't exist
    pub clone: bool,
    /// Configure the repository (run CONFIG_CMD)
    pub configure: bool,
    /// Update the remote URL
    pub set_remote: bool,
    /// Run git commands in the repository
    pub git: bool,
    /// Create a new repository
    pub new: bool,
    /// Debug mode
    pub debug: bool,
    /// Recurse into subdirectories with listfiles
    pub recurse: bool,
    /// List remote relative paths
    pub list_rrel: bool,
    /// List remote URLs
    pub list_rurl: bool,
    /// List local relative paths
    pub list_lrel: bool,
}

/// Primary operation modes that determine the main behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PrimaryMode {
    /// Clone repositories
    Clone,
    /// Execute git commands
    Git,
    /// Update remote URL
    #[clap(name = "set-remote")]
    SetRemote,
    /// Configure repositories 
    Configure,
    /// List remote relative paths
    #[clap(name = "list-rrel")]
    ListRrel,
    /// List remote URLs
    #[clap(name = "list-rurl")]
    ListRurl,
    /// List local relative paths
    #[clap(name = "list-lrel")]
    ListLrel,
    /// Run with clone and set-remote
    Run,
    /// Create new repositories
    New,
}

impl From<PrimaryMode> for Operations {
    fn from(mode: PrimaryMode) -> Self {
        let mut ops = Operations::default();
        match mode {
            PrimaryMode::Clone => {
                ops.clone = true;
                ops.configure = true;
                ops.recurse = true;
            },
            PrimaryMode::Git => {
                ops.git = true;
                ops.set_remote = true;
                ops.configure = true;
                ops.recurse = true;
            },
            PrimaryMode::SetRemote => {
                ops.set_remote = true;
                ops.recurse = true;
            },
            PrimaryMode::Configure => {
                ops.configure = true;
                ops.recurse = true;
            },
            PrimaryMode::ListRrel => {
                ops.list_rrel = true;
                ops.recurse = true;
            },
            PrimaryMode::ListRurl => {
                ops.list_rurl = true;
                ops.recurse = true;
            },
            PrimaryMode::ListLrel => {
                ops.list_lrel = true;
                ops.recurse = true;
            },
            PrimaryMode::Run => {
                ops.clone = true;
                ops.set_remote = true;
                ops.configure = true;
                ops.recurse = true;
            },
            PrimaryMode::New => {
                ops.new = true;
                ops.recurse = true;
            },
        }
        ops
    }
}

impl std::fmt::Display for PrimaryMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrimaryMode::Clone => write!(f, "clone"),
            PrimaryMode::Git => write!(f, "git"),
            PrimaryMode::SetRemote => write!(f, "set-remote"),
            PrimaryMode::Configure => write!(f, "configure"),
            PrimaryMode::ListRrel => write!(f, "list-rrel"),
            PrimaryMode::ListRurl => write!(f, "list-rurl"),
            PrimaryMode::ListLrel => write!(f, "list-lrel"),
            PrimaryMode::Run => write!(f, "run"),
            PrimaryMode::New => write!(f, "new"),
        }
    }
}

/// Global OPERATIONS initialized once at startup
static OPERATIONS: OnceCell<Operations> = OnceCell::new();

/// Initialize the global operations - call this ONCE at startup
pub fn initialize_operations(primary_mode: PrimaryMode) {
    // Create Operations from the primary mode
    let operations = Operations::from(primary_mode);
    
    // Initialize the global operations once
    // If this fails, it means initialize_operations was called more than once
    OPERATIONS.set(operations).expect("OPERATIONS already initialized");
}

/// Get a reference to the operations
/// Panics if initialize_operations wasn't called first
pub fn get_operations() -> &'static Operations {
    OPERATIONS.get().expect("OPERATIONS not initialized")
}

impl Operations {
    /// Check if this is a listing mode
    pub fn is_listing_mode(&self) -> bool {
        self.list_rrel || self.list_rurl || self.list_lrel
    }
}