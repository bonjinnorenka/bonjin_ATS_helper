use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushPolicy {
    WriteThrough,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityMode {
    Fast,
    Durable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockOptions {
    pub(crate) root_path: PathBuf,
    pub(crate) auto_create_if_missing: bool,
    pub(crate) strict_mode: bool,
    pub(crate) flush_policy: FlushPolicy,
    pub(crate) durability_mode: DurabilityMode,
}

impl MockOptions {
    pub fn new(root_path: impl Into<PathBuf>) -> Self {
        Self {
            root_path: root_path.into(),
            auto_create_if_missing: true,
            strict_mode: true,
            flush_policy: FlushPolicy::WriteThrough,
            durability_mode: DurabilityMode::Fast,
        }
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub fn with_auto_create_if_missing(mut self, enabled: bool) -> Self {
        self.auto_create_if_missing = enabled;
        self
    }

    pub fn with_strict_mode(mut self, enabled: bool) -> Self {
        self.strict_mode = enabled;
        self
    }

    pub fn with_flush_policy(mut self, flush_policy: FlushPolicy) -> Self {
        self.flush_policy = flush_policy;
        self
    }

    pub fn with_durability_mode(mut self, durability_mode: DurabilityMode) -> Self {
        self.durability_mode = durability_mode;
        self
    }
}
