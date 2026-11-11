use super::*;

fn step(tool_calls: usize) -> BlockStep {
    BlockStep {
        is_agent_block: true,
        has_answer_text: false,
        tool_calls,
        is_passive: false,
    }
}

fn answer_step() -> BlockStep {
    BlockStep {
        is_agent_block: true,
        has_answer_text: true,
        tool_calls: 0,
        is_passive: false,
    }
}

fn user_block() -> BlockStep {
    BlockStep {
        is_agent_block: false,
        has_answer_text: false,
        tool_calls: 0,
        is_passive: false,
    }
}

#[test]
fn consecutive_tool_steps_form_one_run() {
    let steps = vec![step(1), step(2), step(1)];

    let runs = compute_block_runs(&steps);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].members, vec![0, 1, 2]);
}

#[test]
fn answer_text_ends_the_run() {
    let steps = vec![step(1), step(1), answer_step(), step(1), step(1)];

    let runs = compute_block_runs(&steps);

    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].members, vec![0, 1]);
    assert_eq!(runs[1].members, vec![3, 4]);
}

#[test]
fn user_input_splits_runs() {
    let steps = vec![step(1), step(1), user_block(), step(1), step(1)];

    let runs = compute_block_runs(&steps);

    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].members, vec![0, 1]);
    assert_eq!(runs[1].members, vec![3, 4]);
}

#[test]
fn single_step_is_not_a_run() {
    let steps = vec![answer_step(), step(1), answer_step()];

    assert!(compute_block_runs(&steps).is_empty());
}

#[test]
fn passive_blocks_never_group() {
    let mut passive = step(1);
    passive.is_passive = true;
    let steps = vec![passive, passive, passive];

    assert!(compute_block_runs(&steps).is_empty());
}

#[test]
fn blocks_without_tools_break_the_run() {
    let steps = vec![step(1), step(0), step(1)];

    assert!(compute_block_runs(&steps).is_empty());
}

#[test]
fn run_starting_at_finds_matches() {
    let steps = vec![step(1), step(1), user_block()];
    let runs = compute_block_runs(&steps);

    assert!(run_starting_at(&runs, 0).is_some());
    assert!(run_starting_at(&runs, 1).is_none());
}
