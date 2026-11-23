//! Describer backends: something that turns a prompt into a reply. Real ones shell out to a
//! model CLI, so no HTTP client or API key handling lives here.

use std::fmt;
use std::io::Write;
use std::process::{Command, Stdio};

/// A model that answers one prompt with one reply.
pub trait Backend {
    /// Sends `prompt` and returns the model's raw reply.
    fn complete(&self, prompt: &str) -> Result<String, BackendError>;
}

#[derive(Debug)]
pub enum BackendError {
    /// The program could not be started, usually because it is not installed.
    Spawn {
        program: String,
        source: std::io::Error,
    },
    /// Talking to the running program failed.
    Io(std::io::Error),
    /// The program ran and exited unsuccessfully.
    Failed {
        program: String,
        status: Option<i32>,
        stderr: String,
    },
    /// The program printed something that is not UTF-8 text.
    NotText { program: String },
    /// An unknown backend name was requested.
    UnknownBackend(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Spawn { program, source } => {
                write!(formatter, "cannot start `{program}`: {source}")
            }
            BackendError::Io(error) => write!(formatter, "describer backend io error: {error}"),
            BackendError::Failed {
                program,
                status,
                stderr,
            } => match status {
                Some(code) => write!(formatter, "`{program}` exited with status {code}: {stderr}"),
                None => write!(formatter, "`{program}` was killed by a signal: {stderr}"),
            },
            BackendError::NotText { program } => {
                write!(
                    formatter,
                    "`{program}` printed output that is not UTF-8 text"
                )
            }
            BackendError::UnknownBackend(name) => write!(
                formatter,
                "unknown describer backend `{name}`; use claude, codex or ollama:<model>"
            ),
        }
    }
}

impl std::error::Error for BackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BackendError::Spawn { source, .. } => Some(source),
            BackendError::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// A model CLI that reads the prompt on stdin and prints the reply on stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessBackend {
    program: String,
    args: Vec<String>,
}

impl ProcessBackend {
    /// `claude -p`, which reads the prompt from stdin when none is given as an argument.
    pub fn claude() -> Self {
        Self::new("claude", &["-p"])
    }

    /// `codex exec -`, where `-` forces the prompt to be read from stdin.
    pub fn codex() -> Self {
        Self::new("codex", &["exec", "-"])
    }

    /// `ollama run <model>` with the prompt piped in; fully offline.
    pub fn ollama(model: &str) -> Self {
        Self::new("ollama", &["run", model])
    }

    /// Picks a backend from its command-line name: `claude`, `codex` or `ollama:<model>`.
    pub fn from_name(name: &str) -> Result<Self, BackendError> {
        match name.split_once(':') {
            None if name == "claude" => Ok(Self::claude()),
            None if name == "codex" => Ok(Self::codex()),
            Some(("ollama", model)) if !model.is_empty() => Ok(Self::ollama(model)),
            _ => Err(BackendError::UnknownBackend(name.to_string())),
        }
    }

    fn new(program: &str, args: &[&str]) -> Self {
        ProcessBackend {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        }
    }
}

impl Backend for ProcessBackend {
    fn complete(&self, prompt: &str) -> Result<String, BackendError> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| BackendError::Spawn {
                program: self.program.clone(),
                source,
            })?;
        let mut stdin = child.stdin.take().expect("stdin was requested as a pipe");
        // Written from a thread so a program that prints before it finishes reading cannot
        // deadlock against a full pipe.
        let (write_result, output) = std::thread::scope(|scope| {
            let writer = scope.spawn(move || stdin.write_all(prompt.as_bytes()));
            let output = child.wait_with_output();
            (writer.join().expect("stdin writer does not panic"), output)
        });
        let output = output.map_err(BackendError::Io)?;
        if !output.status.success() {
            return Err(BackendError::Failed {
                program: self.program.clone(),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        // A broken pipe after a clean exit just means the program stopped reading early.
        if let Err(error) = write_result
            && error.kind() != std::io::ErrorKind::BrokenPipe
        {
            return Err(BackendError::Io(error));
        }
        String::from_utf8(output.stdout).map_err(|_| BackendError::NotText {
            program: self.program.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_select_the_command_line_of_each_backend() {
        assert_eq!(
            ProcessBackend::from_name("claude").unwrap(),
            ProcessBackend::claude()
        );
        assert_eq!(
            ProcessBackend::from_name("codex").unwrap(),
            ProcessBackend::codex()
        );
        assert_eq!(
            ProcessBackend::from_name("ollama:llama3").unwrap(),
            ProcessBackend::ollama("llama3")
        );
    }

    #[test]
    fn unknown_or_incomplete_names_are_rejected() {
        for name in ["gpt", "ollama", "ollama:", "claude:opus"] {
            assert!(matches!(
                ProcessBackend::from_name(name),
                Err(BackendError::UnknownBackend(_))
            ));
        }
    }

    #[test]
    fn the_prompt_goes_to_stdin_and_the_reply_comes_from_stdout() {
        let backend = ProcessBackend::new("cat", &[]);
        assert_eq!(backend.complete("hello model").unwrap(), "hello model");
    }

    #[test]
    fn a_failing_program_reports_its_status_and_stderr() {
        let backend = ProcessBackend::new("sh", &["-c", "echo boom >&2; exit 3"]);
        match backend.complete("x") {
            Err(BackendError::Failed { status, stderr, .. }) => {
                assert_eq!(status, Some(3));
                assert_eq!(stderr, "boom");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_program_is_a_spawn_error() {
        let backend = ProcessBackend::new("cartograph-no-such-model", &[]);
        assert!(matches!(
            backend.complete("x"),
            Err(BackendError::Spawn { .. })
        ));
    }
}
