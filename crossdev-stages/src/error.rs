use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("sandbox not found: {0}")]
    SandboxNotFound(String),

    #[error("target not found: {0}")]
    TargetNotFound(String),

    #[error("board not found: {0}")]
    BoardNotFound(String),

    #[error("board config parse error in {file}: {msg}")]
    BoardConfigParse { file: String, msg: String },

    #[error("unknown architecture: {0}")]
    UnknownArch(String),

    #[error("stage3 error: {0}")]
    Stage(#[from] gentoo_stages::Error),

    #[error("container error: {0}")]
    Container(#[from] hakoniwa::Error),

    #[error("command failed (exit {code}): {reason}")]
    CommandFailed { code: i32, reason: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Convert a hakoniwa ExitStatus into a Result, propagating failure.
pub fn check_status(status: hakoniwa::ExitStatus) -> crate::error::Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(Error::CommandFailed {
            code: status.code,
            reason: status.reason.clone(),
        })
    }
}
