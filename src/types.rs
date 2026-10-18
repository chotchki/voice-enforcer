use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Block,
    Warn,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Block => "block",
            Severity::Warn => "warn",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Long-form prose (markdown, plain text) vs code comments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Register {
    LongForm,
    Comment,
}

/// Where a comment block sits, which sets its sentence budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Level {
    Line,
    Item,
    Module,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Line => "line",
            Level::Item => "item",
            Level::Module => "module",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(Severity::Block.to_string(), "block");
        assert_eq!(Severity::Warn.to_string(), "warn");
        assert!(Severity::Block < Severity::Warn, "blocks sort first");
        assert_eq!(Level::Line.as_str(), "line");
        assert_eq!(Level::Item.as_str(), "item");
        assert_eq!(Level::Module.as_str(), "module");
    }
}
