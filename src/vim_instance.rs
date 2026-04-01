use crate::state::InputMode;
use crate::vim_buffer::VimBuffer;

/// Configures which Vim modes are available in an instance
#[derive(Debug, Clone, Copy)]
pub struct VimModeConfig {
    pub normal: bool,
    pub insert: bool,
    pub visual: bool,
    pub visual_block: bool,
}

#[allow(dead_code)]
impl VimModeConfig {
    /// All modes enabled
    pub fn full() -> Self {
        Self {
            normal: true,
            insert: true,
            visual: true,
            visual_block: true,
        }
    }

    /// No insertion (ideal for response body viewer)
    pub fn read_only_visual() -> Self {
        Self {
            normal: true,
            insert: false,
            visual: true,
            visual_block: true,
        }
    }

    pub fn with_insert(mut self, enabled: bool) -> Self {
        self.insert = enabled;
        self
    }

    pub fn with_normal(mut self, enabled: bool) -> Self {
        self.normal = enabled;
        self
    }

    pub fn with_visual(mut self, enabled: bool) -> Self {
        self.visual = enabled;
        self
    }

    pub fn with_visual_block(mut self, enabled: bool) -> Self {
        self.visual_block = enabled;
        self
    }

    /// Check if a mode is allowed
    pub fn is_mode_allowed(&self, mode: InputMode) -> bool {
        match mode {
            InputMode::Normal => self.normal,
            InputMode::Insert => self.insert,
            InputMode::Visual => self.visual,
            InputMode::VisualBlock => self.visual_block,
        }
    }
}

/// VimInstance: Combines VimBuffer + mode configuration
#[derive(Debug)]
pub struct VimInstance {
    pub buffer: VimBuffer,
    pub config: VimModeConfig,
}

#[allow(dead_code)]
impl VimInstance {
    /// Create with all modes active
    pub fn new() -> Self {
        Self {
            buffer: VimBuffer::default(),
            config: VimModeConfig::full(),
        }
    }

    /// Create for read-only (no insertion)
    pub fn read_only() -> Self {
        Self {
            buffer: VimBuffer::default(),
            config: VimModeConfig::read_only_visual(),
        }
    }

    /// Create with custom config
    pub fn with_config(config: VimModeConfig) -> Self {
        Self {
            buffer: VimBuffer::default(),
            config,
        }
    }

    /// Check if a mode is allowed
    pub fn is_mode_allowed(&self, mode: InputMode) -> bool {
        self.config.is_mode_allowed(mode)
    }

    /// Attempt to change mode - respects configuration
    pub fn try_set_mode(&self, new_mode: InputMode) -> bool {
        self.config.is_mode_allowed(new_mode)
    }
}

impl Default for VimInstance {
    fn default() -> Self {
        Self::new()
    }
}
