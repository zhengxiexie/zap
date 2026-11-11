use super::*;
use crate::ai::agent::{
    task::TaskId, AIAgentAction, AIAgentActionId, AIAgentActionType, AIAgentOutputMessage,
    AIAgentOutputMessageType, AIAgentText, MessageId, ReadFilesRequest, WebSearchStatus,
};

fn action_message(id: &str, action: AIAgentActionType) -> AIAgentOutputMessage {
    AIAgentOutputMessage {
        id: MessageId::new(id.to_string()),
        message: AIAgentOutputMessageType::Action(AIAgentAction {
            id: AIAgentActionId::from(id.to_string()),
            task_id: TaskId::new("test-task".to_string()),
            action,
            requires_result: false,
        }),
        citations: vec![],
    }
}

fn non_tool_message(id: &str) -> AIAgentOutputMessage {
    AIAgentOutputMessage {
        id: MessageId::new(id.to_string()),
        message: AIAgentOutputMessageType::DebugOutput {
            text: String::new(),
        },
        citations: vec![],
    }
}

fn read_action() -> AIAgentActionType {
    AIAgentActionType::ReadFiles(ReadFilesRequest { locations: vec![] })
}

fn grep_action() -> AIAgentActionType {
    AIAgentActionType::Grep {
        queries: vec!["TODO".to_string()],
        path: ".".to_string(),
    }
}

fn command_action() -> AIAgentActionType {
    AIAgentActionType::RequestCommandOutput {
        command: "ls".to_string(),
        is_read_only: None,
        is_risky: None,
        wait_until_completion: true,
        uses_pager: None,
        rationale: None,
        citations: vec![],
    }
}

fn edit_action() -> AIAgentActionType {
    AIAgentActionType::RequestFileEdits {
        file_edits: vec![],
        title: None,
    }
}

fn mcp_action() -> AIAgentActionType {
    AIAgentActionType::CallMCPTool {
        server_id: None,
        name: "search".to_string(),
        input: serde_json::Value::Null,
    }
}

fn ask_question_action() -> AIAgentActionType {
    AIAgentActionType::AskUserQuestion { questions: vec![] }
}

fn web_search_message(id: &str) -> AIAgentOutputMessage {
    AIAgentOutputMessage {
        id: MessageId::new(id.to_string()),
        message: AIAgentOutputMessageType::WebSearch(WebSearchStatus::Searching { query: None }),
        citations: vec![],
    }
}

fn reasoning_message(id: &str) -> AIAgentOutputMessage {
    AIAgentOutputMessage {
        id: MessageId::new(id.to_string()),
        message: AIAgentOutputMessageType::Reasoning {
            text: AIAgentText { sections: vec![] },
            finished_duration: None,
        },
        citations: vec![],
    }
}

#[test]
fn consecutive_actions_form_one_group() {
    let messages = vec![
        action_message("a", read_action()),
        action_message("b", grep_action()),
        action_message("c", command_action()),
        action_message("d", edit_action()),
    ];

    let groups = compute_action_groups(&messages);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].member_indices, vec![0, 1, 2, 3]);
    assert_eq!(
        groups[0].counts,
        ActionGroupCounts {
            edits: 1,
            commands: 1,
            reads: 2,
            tools: 0,
        }
    );
}

#[test]
fn reasoning_between_actions_does_not_split_groups() {
    let messages = vec![
        reasoning_message("r1"),
        action_message("a", read_action()),
        reasoning_message("r2"),
        action_message("b", command_action()),
        reasoning_message("r3"),
        action_message("c", grep_action()),
    ];

    let groups = compute_action_groups(&messages);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].first_index, 1);
    assert_eq!(groups[0].member_indices, vec![1, 3, 5]);
    assert_eq!(groups[0].counts.total(), 3);
}

#[test]
fn text_messages_do_not_split_groups() {
    let messages = vec![
        action_message("a", read_action()),
        action_message("b", read_action()),
        non_tool_message("c"),
        action_message("d", grep_action()),
        action_message("e", grep_action()),
    ];

    let groups = compute_action_groups(&messages);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].member_indices, vec![0, 1, 3, 4]);
}

#[test]
fn single_action_is_not_grouped() {
    let messages = vec![action_message("a", read_action()), non_tool_message("b")];

    assert!(compute_action_groups(&messages).is_empty());
}

#[test]
fn interactive_actions_stay_visible_and_outside_the_group() {
    let messages = vec![
        action_message("a", read_action()),
        action_message("b", ask_question_action()),
        action_message("c", read_action()),
    ];

    let groups = compute_action_groups(&messages);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].member_indices, vec![0, 2]);
    assert_eq!(groups[0].counts.total(), 2);
}

#[test]
fn counts_classify_by_kind() {
    let messages = vec![
        action_message("a", edit_action()),
        action_message("b", command_action()),
        action_message("c", read_action()),
        action_message("d", grep_action()),
        action_message("e", mcp_action()),
    ];

    let groups = compute_action_groups(&messages);

    assert_eq!(
        groups[0].counts,
        ActionGroupCounts {
            edits: 1,
            commands: 1,
            reads: 2,
            tools: 1,
        }
    );
}

#[test]
fn web_search_counts_as_tool() {
    let messages = vec![web_search_message("a"), web_search_message("b")];

    let groups = compute_action_groups(&messages);

    assert_eq!(groups[0].counts.tools, 2);
    assert_eq!(groups[0].counts.total(), 2);
}

#[test]
fn summary_text_skips_zero_and_pluralizes() {
    let counts = ActionGroupCounts {
        edits: 1,
        commands: 2,
        reads: 0,
        tools: 0,
    };

    assert_eq!(counts.summary_text(), "1 edit · 2 commands");
}

#[test]
fn summary_text_matches_tivano_example() {
    let counts = ActionGroupCounts {
        edits: 4,
        commands: 4,
        reads: 6,
        tools: 0,
    };

    assert_eq!(counts.summary_text(), "4 edits · 4 commands · 6 reads");
}

#[test]
fn title_uses_dominant_categories() {
    let counts = ActionGroupCounts {
        edits: 0,
        commands: 6,
        reads: 2,
        tools: 0,
    };

    assert_eq!(counts.title(false), "Running commands, Reading files");
    assert_eq!(counts.title(true), "Ran commands, Read files");
}

#[test]
fn title_breaks_ties_by_fixed_order() {
    let counts = ActionGroupCounts {
        edits: 2,
        commands: 2,
        reads: 0,
        tools: 0,
    };

    assert_eq!(counts.title(true), "Edited files, Ran commands");
}

#[test]
fn group_id_derives_from_first_message() {
    let messages = vec![
        non_tool_message("x"),
        action_message("msg-1", read_action()),
        action_message("msg-2", read_action()),
    ];

    let groups = compute_action_groups(&messages);

    assert_eq!(groups[0].id, MessageId::new("group:msg-1".to_string()));
    assert_eq!(groups[0].member_indices, vec![1, 2]);
}

#[test]
fn group_starting_at_finds_matches() {
    let messages = vec![
        action_message("a", read_action()),
        action_message("b", read_action()),
        non_tool_message("c"),
    ];
    let groups = compute_action_groups(&messages);

    assert!(group_starting_at(&groups, 0).is_some());
    assert!(group_starting_at(&groups, 1).is_none());
    assert!(group_starting_at(&groups, 2).is_none());
}
