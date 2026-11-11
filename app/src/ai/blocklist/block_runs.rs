//! Groups consecutive agent-step blocks so a long turn can collapse into a
//! single summary card. One agent step = one `AIAgentOutput` = one block, so
//! grouping has to happen at the block-list level rather than inside an output.

use crate::ai::agent::{AIAgentExchange, AIAgentExchangeId};
use crate::ai::blocklist::block::view_impl::action_groups::{self, ActionGroupCounts};

/// What the block list knows about a single block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockStep {
    pub is_agent_block: bool,
    /// The block contains user-visible answer text, so it ends a run.
    pub has_answer_text: bool,
    /// Number of tool calls in the block.
    pub tool_calls: usize,
    /// Passive suggestions and other non-conversation blocks never group.
    pub is_passive: bool,
}

/// A run of consecutive agent-step blocks collapsed into one card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRun {
    pub first: usize,
    pub members: Vec<usize>,
}

impl BlockRun {
    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn members_after_first(&self) -> impl Iterator<Item = usize> + '_ {
        self.members.iter().skip(1).copied()
    }
}

/// Runs shorter than this stay as-is.
pub const MIN_STEPS_PER_RUN: usize = 2;

fn is_candidate(step: &BlockStep) -> bool {
    step.is_agent_block && !step.is_passive && !step.has_answer_text && step.tool_calls > 0
}

/// Splits blocks into runs of consecutive tool-only agent steps. A block that
/// contains answer text, a user input, or any non-agent block ends the run.
pub fn compute_block_runs(steps: &[BlockStep]) -> Vec<BlockRun> {
    let mut runs = Vec::new();
    let mut members = Vec::new();

    let flush = |members: &mut Vec<usize>, runs: &mut Vec<BlockRun>| {
        if members.len() >= MIN_STEPS_PER_RUN {
            runs.push(BlockRun {
                first: members[0],
                members: std::mem::take(members),
            });
        } else {
            members.clear();
        }
    };

    for (index, step) in steps.iter().enumerate() {
        if is_candidate(step) {
            members.push(index);
        } else {
            flush(&mut members, &mut runs);
        }
    }
    flush(&mut members, &mut runs);

    runs
}

/// Returns the run that starts at `block_index`, if any.
pub fn run_starting_at(runs: &[BlockRun], block_index: usize) -> Option<&BlockRun> {
    runs.iter().find(|run| run.first == block_index)
}

/// What a single agent step contributed to a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentStepSummary {
    pub counts: ActionGroupCounts,
    pub has_answer_text: bool,
}

impl AgentStepSummary {
    pub fn to_step(self, is_passive: bool) -> BlockStep {
        BlockStep {
            is_agent_block: true,
            has_answer_text: self.has_answer_text,
            tool_calls: self.counts.total(),
            is_passive,
        }
    }
}

/// Summarizes the output of one exchange.
pub fn summarize_exchange(exchange: &AIAgentExchange) -> AgentStepSummary {
    let Some(output) = exchange.output_status.output() else {
        return AgentStepSummary::default();
    };
    let messages = &output.get().messages;
    AgentStepSummary {
        counts: action_groups::summarize_step(messages),
        has_answer_text: action_groups::ends_with_answer_text(messages),
    }
}

/// A collapsed run, as seen from one of its member steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentRunInfo {
    /// Index of the run's first step.
    pub head_index: usize,
    /// Exchange id of the run's first step, used to look up its view.
    pub head_exchange_id: AIAgentExchangeId,
    /// Index of this step in the conversation.
    pub index: usize,
    /// Number of steps in the run.
    pub step_count: usize,
    /// Tool calls across every step in the run.
    pub totals: ActionGroupCounts,
    /// Tool calls made by this step alone.
    pub step_counts: ActionGroupCounts,
    /// 1-based position of this step within the run.
    pub position: usize,
    pub is_head: bool,
}

/// Resolves the run that contains `index`, if any.
pub fn agent_run_containing(
    steps: &[BlockStep],
    summaries: &[AgentStepSummary],
    exchange_ids: &[AIAgentExchangeId],
    index: usize,
) -> Option<AgentRunInfo> {
    let runs = compute_block_runs(steps);
    let run = runs.iter().find(|run| run.members.contains(&index))?;
    let mut totals = ActionGroupCounts::default();
    for member in &run.members {
        totals.merge(&summaries[*member].counts);
    }
    Some(AgentRunInfo {
        head_index: run.first,
        head_exchange_id: exchange_ids
            .get(run.first)
            .copied()
            .unwrap_or(exchange_ids[index]),
        index,
        step_count: run.len(),
        totals,
        step_counts: summaries[index].counts,
        position: index - run.first + 1,
        is_head: run.first == index,
    })
}

#[cfg(test)]
#[path = "block_runs_tests.rs"]
mod tests;
