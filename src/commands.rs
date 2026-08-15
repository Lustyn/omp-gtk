use crate::bridge::protocol::{SlashCommand, SlashSubcommand};

const COMPLETION_LIMIT: usize = 9;

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
            .filter(|command| command_matches(command, &query))
            .take(COMPLETION_LIMIT)
            .map(command_completion)
            .collect();
    };

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

    use super::completions;

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
    fn ignores_regular_multiline_text() {
        assert!(completions("hello", &commands()).is_empty());
        assert!(completions("/todo\nappend", &commands()).is_empty());
    }
}
