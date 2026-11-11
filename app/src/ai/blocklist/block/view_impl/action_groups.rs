//! Groups consecutive tool-call output messages into one collapsible card.

use crate::ai::agent::{
    AIAgentActionType, AIAgentOutputMessage, AIAgentOutputMessageType, MessageId,
};

/// Minimum number of consecutive tool messages before they are collapsed into a group.
pub const MIN_ACTIONS_PER_GROUP: usize = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActionGroupCounts {
    pub edits: usize,
    pub commands: usize,
    pub reads: usize,
    pub tools: usize,
}

/// Ordering doubles as the tie-breaker when two categories have the same count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ActionCategory {
    Edits,
    Commands,
    Reads,
    Tools,
}

impl ActionCategory {
    fn label(self, is_finished: bool) -> &'static str {
        match (self, is_finished) {
            (Self::Edits, true) => "Edited files",
            (Self::Edits, false) => "Editing files",
            (Self::Commands, true) => "Ran commands",
            (Self::Commands, false) => "Running commands",
            (Self::Reads, true) => "Read files",
            (Self::Reads, false) => "Reading files",
            (Self::Tools, true) => "Used tools",
            (Self::Tools, false) => "Using tools",
        }
    }
}

impl ActionGroupCounts {
    pub fn total(&self) -> usize {
        self.edits + self.commands + self.reads + self.tools
    }

    /// Adds another step's counts, for cross-block totals.
    pub fn merge(&mut self, other: &Self) {
        self.edits += other.edits;
        self.commands += other.commands;
        self.reads += other.reads;
        self.tools += other.tools;
    }

    fn record(&mut self, action: &AIAgentActionType) {
        match action {
            AIAgentActionType::RequestFileEdits { .. } => self.edits += 1,
            AIAgentActionType::RequestCommandOutput { .. }
            | AIAgentActionType::WriteToLongRunningShellCommand { .. }
            | AIAgentActionType::ReadShellCommandOutput { .. }
            | AIAgentActionType::TransferShellCommandControlToUser { .. } => self.commands += 1,
            AIAgentActionType::ReadFiles(..)
            | AIAgentActionType::Grep { .. }
            | AIAgentActionType::FileGlob { .. }
            | AIAgentActionType::FileGlobV2 { .. }
            | AIAgentActionType::ReadMCPResource { .. }
            | AIAgentActionType::ReadSkill(..) => self.reads += 1,
            _ => self.tools += 1,
        }
    }

    fn entries(&self) -> [(ActionCategory, usize); 4] {
        [
            (ActionCategory::Edits, self.edits),
            (ActionCategory::Commands, self.commands),
            (ActionCategory::Reads, self.reads),
            (ActionCategory::Tools, self.tools),
        ]
    }

    fn dominant_categories(&self) -> Vec<ActionCategory> {
        let mut entries: Vec<(ActionCategory, usize)> = self
            .entries()
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        entries.into_iter().map(|(category, _)| category).collect()
    }

    /// Card title, e.g. `Read files, running commands` while streaming and
    /// `Read files, ran commands` once finished.
    pub fn title(&self, is_finished: bool) -> String {
        let segments: Vec<&str> = self
            .dominant_categories()
            .into_iter()
            .take(2)
            .map(|category| category.label(is_finished))
            .collect();
        if segments.is_empty() {
            return ActionCategory::Tools.label(is_finished).to_string();
        }
        segments.join(", ")
    }

    /// Summary line, e.g. `4 edits · 4 commands · 6 reads`.
    pub fn summary_text(&self) -> String {
        let mut parts = Vec::new();
        for (count, singular, plural) in [
            (self.edits, "edit", "edits"),
            (self.commands, "command", "commands"),
            (self.reads, "read", "reads"),
            (self.tools, "tool", "tools"),
        ] {
            if count == 0 {
                continue;
            }
            let noun = if count == 1 { singular } else { plural };
            parts.push(format!("{count} {noun}"));
        }
        parts.join(" · ")
    }
}

/// A run of consecutive tool-call output messages rendered as one card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionGroup {
    /// Synthetic, stable id: `group:<id of the first message in the run>`.
    pub id: MessageId,
    /// Index of the first tool message; the card is rendered here.
    pub first_index: usize,
    /// Indices of every tool message in the group, in order.
    pub member_indices: Vec<usize>,
    pub counts: ActionGroupCounts,
}

impl ActionGroup {
    pub fn len(&self) -> usize {
        self.member_indices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.member_indices.is_empty()
    }

    pub fn contains(&self, message_index: usize) -> bool {
        self.member_indices.contains(&message_index)
    }

    /// Tool messages after the first one; hidden while collapsed.
    pub fn members_after_first(&self) -> impl Iterator<Item = usize> + '_ {
        self.member_indices.iter().skip(1).copied()
    }

    pub fn title(&self, is_finished: bool) -> String {
        self.counts.title(is_finished)
    }

    pub fn summary_text(&self) -> String {
        self.counts.summary_text()
    }
}

/// Actions that must stay visible: they block on the user and cannot be hidden
/// behind a collapsed card.
fn is_interactive(action: &AIAgentActionType) -> bool {
    matches!(
        action,
        AIAgentActionType::AskUserQuestion { .. }
            | AIAgentActionType::SuggestPrompt(..)
            | AIAgentActionType::SuggestNewConversation { .. }
            | AIAgentActionType::OpenCodeReview
            | AIAgentActionType::InsertCodeReviewComments { .. }
    )
}

/// How a message participates in grouping.
enum GroupMember<'a> {
    Action(&'a AIAgentActionType),
    Tool,
}

/// Returns `None` for messages that are neither tool calls nor boundaries.
/// Text and reasoning do **not** break a group: a turn with interleaved
/// narration should still collapse into one summary card, like Tivano.
fn classify_message(message: &AIAgentOutputMessage) -> Option<GroupMember<'_>> {
    match &message.message {
        AIAgentOutputMessageType::Action(action) if !is_interactive(&action.action) => {
            Some(GroupMember::Action(&action.action))
        }
        AIAgentOutputMessageType::WebSearch(_)
        | AIAgentOutputMessageType::WebFetch(_)
        | AIAgentOutputMessageType::TodoOperation(_) => Some(GroupMember::Tool),
        _ => None,
    }
}

/// Splits `messages` into tool-call runs. A run is every non-interactive tool
/// message in the turn; text and reasoning messages are transparent so a step
/// that narrates between tool calls still collapses into one card.
pub fn compute_action_groups(messages: &[AIAgentOutputMessage]) -> Vec<ActionGroup> {
    let mut member_indices = Vec::new();
    let mut counts = ActionGroupCounts::default();

    for (index, message) in messages.iter().enumerate() {
        match classify_message(message) {
            Some(GroupMember::Action(action)) => {
                counts.record(action);
                member_indices.push(index);
            }
            Some(GroupMember::Tool) => {
                counts.tools += 1;
                member_indices.push(index);
            }
            None => {}
        }
    }

    if member_indices.len() < MIN_ACTIONS_PER_GROUP {
        return Vec::new();
    }

    vec![ActionGroup {
        id: MessageId::new(format!("group:{}", &*messages[member_indices[0]].id)),
        first_index: member_indices[0],
        member_indices,
        counts,
    }]
}

/// Returns the group that starts at `message_index`, if any.
pub fn group_starting_at(groups: &[ActionGroup], message_index: usize) -> Option<&ActionGroup> {
    groups
        .iter()
        .find(|group| group.first_index == message_index)
}

/// Counts the tool calls in a step. Also used for cross-block grouping, where
/// each block is a single agent step.
pub fn summarize_step(messages: &[AIAgentOutputMessage]) -> ActionGroupCounts {
    let mut counts = ActionGroupCounts::default();
    for message in messages {
        match classify_message(message) {
            Some(GroupMember::Action(action)) => counts.record(action),
            Some(GroupMember::Tool) => counts.tools += 1,
            None => {}
        }
    }
    counts
}

/// `true` when the last meaningful thing the step did was emit answer text.
///
/// Trailing text ends a collapsible run; narration between tool calls is
/// followed by another tool call and therefore does not.
pub fn ends_with_answer_text(messages: &[AIAgentOutputMessage]) -> bool {
    let mut ends_with_text = false;
    for message in messages {
        match &message.message {
            AIAgentOutputMessageType::Text(text) => {
                if !text.sections.iter().all(|section| section.is_empty()) {
                    ends_with_text = true;
                }
            }
            AIAgentOutputMessageType::Action(_)
            | AIAgentOutputMessageType::WebSearch(_)
            | AIAgentOutputMessageType::WebFetch(_)
            | AIAgentOutputMessageType::TodoOperation(_) => ends_with_text = false,
            _ => {}
        }
    }
    ends_with_text
}

#[cfg(test)]
#[path = "action_groups_tests.rs"]
mod tests;
