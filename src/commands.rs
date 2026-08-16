use crate::bridge::protocol::{SlashCommand, SlashSubcommand};

const COMPLETION_LIMIT: usize = 9;

const UNSUPPORTED_NATIVE_MODE_COMMANDS: [&str; 4] = ["vibe", "goal", "guided-goal", "loop"];

pub fn unsupported_native_mode_command(input: &str) -> Option<&'static str> {
    let command = input
        .trim_start()
        .strip_prefix('/')?
        .split(char::is_whitespace)
        .next()?;
    unsupported_native_mode_name(command)
}

pub fn unsupported_native_mode_error(input: &str) -> Option<String> {
    unsupported_native_mode_command(input).map(|command| {
        format!(
            "The /{command} command is not available in omp native because omp RPC does not \
             expose structured commands and authoritative state for terminal-only modes. The \
             command was not sent to the model."
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCompletion {
    pub label: String,
    pub replacement: String,
    pub description: String,
    pub detail: Option<String>,
}

pub fn completions(input: &str, commands: &[SlashCommand]) -> Vec<CommandCompletion> {
    if !input.starts_with('/') || input.contains('\n') {
        return Vec::new();
    }

    let body = &input[1..];
    let Some(space) = body.find(char::is_whitespace) else {
        let query = body.to_ascii_lowercase();
        return commands
            .iter()
            .filter(|command| !command_is_unsupported_native_mode(command))
            .filter(|command| command_matches(command, &query))
            .take(COMPLETION_LIMIT)
            .map(command_completion)
            .collect();
    };

    if unsupported_native_mode_command(input).is_some() {
        return Vec::new();
    }

    let command_name = &body[..space];
    let Some(command) = commands.iter().find(|command| {
        command.name.eq_ignore_ascii_case(command_name)
            || command
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(command_name))
    }) else {
        return Vec::new();
    };

    let query = body[space..].trim_start().to_ascii_lowercase();
    command
        .subcommands
        .iter()
        .filter(|subcommand| subcommand.name.to_ascii_lowercase().starts_with(&query))
        .take(COMPLETION_LIMIT)
        .map(|subcommand| subcommand_completion(command, subcommand))
        .collect()
}

fn command_is_unsupported_native_mode(command: &SlashCommand) -> bool {
    unsupported_native_mode_name(&command.name).is_some()
        || command
            .aliases
            .iter()
            .any(|alias| unsupported_native_mode_name(alias).is_some())
}

fn unsupported_native_mode_name(name: &str) -> Option<&'static str> {
    UNSUPPORTED_NATIVE_MODE_COMMANDS
        .into_iter()
        .find(|unsupported| name.eq_ignore_ascii_case(unsupported))
}

fn command_matches(command: &SlashCommand, query: &str) -> bool {
    command.name.to_ascii_lowercase().starts_with(query)
        || command
            .aliases
            .iter()
            .any(|alias| alias.to_ascii_lowercase().starts_with(query))
}

fn command_completion(command: &SlashCommand) -> CommandCompletion {
    let accepts_input = command.input.is_some() || !command.subcommands.is_empty();
    CommandCompletion {
        label: format!("/{}", command.name),
        replacement: format!("/{}{}", command.name, if accepts_input { " " } else { "" }),
        description: command.description.clone().unwrap_or_default(),
        detail: command.input.as_ref().and_then(|input| input.hint.clone()),
    }
}

fn subcommand_completion(
    command: &SlashCommand,
    subcommand: &SlashSubcommand,
) -> CommandCompletion {
    CommandCompletion {
        label: format!("/{} {}", command.name, subcommand.name),
        replacement: format!(
            "/{} {}{}",
            command.name,
            subcommand.name,
            if subcommand.usage.is_some() { " " } else { "" }
        ),
        description: subcommand.description.clone().unwrap_or_default(),
        detail: subcommand.usage.clone(),
    }
}

#[cfg(test)]
mod tests {
    use crate::bridge::protocol::{SlashCommand, SlashCommandInput, SlashSubcommand};

    use super::{completions, unsupported_native_mode_command, unsupported_native_mode_error};

    fn commands() -> Vec<SlashCommand> {
        vec![
            SlashCommand {
                name: "model".to_owned(),
                aliases: vec!["models".to_owned()],
                description: Some("Show current model".to_owned()),
                input: None,
                subcommands: Vec::new(),
            },
            SlashCommand {
                name: "todo".to_owned(),
                aliases: Vec::new(),
                description: Some("Manage todos".to_owned()),
                input: Some(SlashCommandInput {
                    hint: Some("<subcommand>".to_owned()),
                }),
                subcommands: vec![SlashSubcommand {
                    name: "append".to_owned(),
                    description: Some("Append a task".to_owned()),
                    usage: Some("[phase] <task>".to_owned()),
                }],
            },
        ]
    }

    fn command(name: &str) -> SlashCommand {
        SlashCommand {
            name: name.to_owned(),
            aliases: Vec::new(),
            description: None,
            input: None,
            subcommands: Vec::new(),
        }
    }

    #[test]
    fn completes_top_level_commands_and_aliases() {
        let top_level = completions("/mo", &commands());
        assert_eq!(top_level[0].replacement, "/model");
        let alias = completions("/models", &commands());
        assert_eq!(alias[0].label, "/model");
    }

    #[test]
    fn completes_subcommands_with_usage_space() {
        let candidates = completions("/todo ap", &commands());
        assert_eq!(candidates[0].replacement, "/todo append ");
        assert_eq!(candidates[0].detail.as_deref(), Some("[phase] <task>"));
    }

    #[test]
    fn never_advertises_terminal_only_mode_commands() {
        let mut goal = command("goal");
        goal.subcommands.push(SlashSubcommand {
            name: "show".to_owned(),
            description: Some("Show current goal details".to_owned()),
            usage: None,
        });
        let mut commands = vec![
            command("vibe"),
            goal,
            command("guided-goal"),
            command("loop"),
        ];
        commands.push(SlashCommand {
            name: "other".to_owned(),
            aliases: vec!["goal".to_owned()],
            description: None,
            input: None,
            subcommands: Vec::new(),
        });

        for input in ["/v", "/go", "/goal ", "/goal sh", "/guided", "/l", "/other"] {
            assert!(completions(input, &commands).is_empty(), "{input}");
        }
    }

    #[test]
    fn identifies_terminal_only_mode_commands_without_prefix_collisions() {
        for (input, expected) in [
            ("/vibe", "vibe"),
            ("/GOAL set ship it", "goal"),
            ("  /guided-goal rough objective", "guided-goal"),
            ("/loop\n5 prompt", "loop"),
        ] {
            assert_eq!(unsupported_native_mode_command(input), Some(expected));
        }

        for input in ["/goalkeeper", "/loops", "please /goal", "/"] {
            assert_eq!(unsupported_native_mode_command(input), None, "{input}");
        }
    }

    #[test]
    fn unsupported_mode_error_explains_that_nothing_was_sent() {
        assert_eq!(
            unsupported_native_mode_error("/goal show").as_deref(),
            Some(
                "The /goal command is not available in omp native because omp RPC does not \
                 expose structured commands and authoritative state for terminal-only modes. \
                 The command was not sent to the model."
            )
        );
    }

    #[test]
    fn ignores_regular_multiline_text() {
        assert!(completions("hello", &commands()).is_empty());
        assert!(completions("/todo\nappend", &commands()).is_empty());
    }
}
