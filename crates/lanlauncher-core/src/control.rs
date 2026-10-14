//! Commands for the running launcher from the command line:
//! `nextgen-lan-launcher --install <id>`, `--uninstall <id>`, `--play <id>`,
//! `--components <id>`, `--status [<id>]`.
//!
//! They do what the buttons do, through the same code, in the launcher that
//! is already running with its sync engine — a second launcher next to it
//! would run a second engine on the same folders. On Linux they travel over
//! the single-instance socket (`instance::send`) and the answer comes back;
//! on Windows and macOS `tauri-plugin-single-instance` hands the arguments
//! over and the answer goes to the log only.

/// One request for the running launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Install(String),
    Uninstall(String),
    /// `skip_components`: start although the profile's Windows components
    /// are missing, as "start without" after the question does.
    Play {
        game_id: String,
        skip_components: bool,
    },
    /// Install the profile's Windows components (winetricks), as the button.
    Components(String),
    /// Every game the launcher tracks, or one.
    Status(Option<String>),
}

/// A game id as the catalog spells them: it goes into one line of the
/// socket's protocol, and nothing else may.
fn is_game_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
}

impl Command {
    /// The command among a start's arguments: `None` for a start without one,
    /// an error for one that is not complete (`--install` without a game).
    pub fn from_args<S: AsRef<str>>(args: &[S]) -> Option<Result<Command, String>> {
        let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
        let at = args.iter().position(|a| {
            matches!(
                *a,
                "--install" | "--uninstall" | "--play" | "--components" | "--status"
            )
        })?;
        // A command goes to the launcher that is running; options for a
        // launcher of its own would be lost on the way.
        if let Some(option) = args
            .iter()
            .find(|a| matches!(**a, "--demo" | "--safe-graphics" | "--no-safe-graphics"))
        {
            return Some(Err(format!(
                "{} goes to the running launcher and cannot be combined with {option}",
                args[at]
            )));
        }
        let id = args.get(at + 1).copied().filter(|id| !id.starts_with("--"));
        let game = |id: Option<&str>| match id {
            Some(id) if is_game_id(id) => Ok(id.to_string()),
            Some(id) => Err(format!("not a game id: {id}")),
            None => Err(format!("{} needs a game id", args[at])),
        };
        Some(match args[at] {
            "--install" => game(id).map(Command::Install),
            "--uninstall" => game(id).map(Command::Uninstall),
            "--components" => game(id).map(Command::Components),
            "--play" => game(id).map(|game_id| Command::Play {
                game_id,
                skip_components: args.contains(&"--skip-components"),
            }),
            _ => match id {
                None => Ok(Command::Status(None)),
                id => game(id).map(|id| Command::Status(Some(id))),
            },
        })
    }

    /// The command as one line of the socket's protocol.
    pub fn to_line(&self) -> String {
        match self {
            Command::Install(id) => format!("install {id}"),
            Command::Uninstall(id) => format!("uninstall {id}"),
            Command::Components(id) => format!("components {id}"),
            Command::Play {
                game_id,
                skip_components,
            } => match skip_components {
                true => format!("play {game_id} skip-components"),
                false => format!("play {game_id}"),
            },
            Command::Status(None) => "status".into(),
            Command::Status(Some(id)) => format!("status {id}"),
        }
    }

    /// What [`Self::to_line`] wrote; `None` for any other line.
    pub fn from_line(line: &str) -> Option<Command> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let id = |at: usize| {
            words
                .get(at)
                .filter(|id| is_game_id(id))
                .map(|id| id.to_string())
        };
        match words.as_slice() {
            ["install", _] => id(1).map(Command::Install),
            ["uninstall", _] => id(1).map(Command::Uninstall),
            ["components", _] => id(1).map(Command::Components),
            ["play", _] => id(1).map(|game_id| Command::Play {
                game_id,
                skip_components: false,
            }),
            ["play", _, "skip-components"] => id(1).map(|game_id| Command::Play {
                game_id,
                skip_components: true,
            }),
            ["status"] => Some(Command::Status(None)),
            ["status", _] => id(1).map(|id| Command::Status(Some(id))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_come_from_the_arguments_and_survive_the_socket() {
        let parse = |args: &[&str]| Command::from_args(args);
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&["--safe-graphics"]), None);
        assert_eq!(
            parse(&["--install", "assetto"]),
            Some(Ok(Command::Install("assetto".into())))
        );
        assert_eq!(
            parse(&["--play", "assetto", "--skip-components"]),
            Some(Ok(Command::Play {
                game_id: "assetto".into(),
                skip_components: true
            }))
        );
        assert_eq!(parse(&["--status"]), Some(Ok(Command::Status(None))));
        assert!(matches!(parse(&["--uninstall"]), Some(Err(_))));
        assert!(matches!(
            parse(&["--demo", "--play", "amongus"]),
            Some(Err(_))
        ));
        assert!(matches!(parse(&["--install", "--status"]), Some(Err(_))));
        // Nothing that would end the line or start another word.
        assert!(matches!(parse(&["--install", "a b"]), Some(Err(_))));
        assert!(matches!(parse(&["--install", "a\nquit"]), Some(Err(_))));

        for command in [
            Command::Install("cod2".into()),
            Command::Uninstall("cod2".into()),
            Command::Components("assetto".into()),
            Command::Play {
                game_id: "flat2".into(),
                skip_components: false,
            },
            Command::Play {
                game_id: "flat2".into(),
                skip_components: true,
            },
            Command::Status(None),
            Command::Status(Some("aoe2".into())),
        ] {
            assert_eq!(Command::from_line(&command.to_line()), Some(command));
        }
        assert_eq!(Command::from_line("show"), None);
        assert_eq!(Command::from_line("install ../x y"), None);
        assert_eq!(Command::from_line("uninstall .."), None);
    }
}
