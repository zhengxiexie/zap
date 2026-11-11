//! 工作区级的 Agent 状态聚合。
//!
//! 将分散在各个 tab 的会话状态汇成一个常驻状态胶囊：多少在跑、
//! 多少在等用户确认、多少出错。Hover 展开逐会话列表，点击跳到对应会话。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::Local;
use pathfinder_geometry::vector::vec2f;
use warpui::elements::{
    Border, ChildAnchor, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Flex,
    Hoverable, MouseStateHandle, OffsetPositioning, ParentAnchor, ParentElement,
    ParentOffsetBounds, Radius, Stack, Text,
};
use warpui::platform::Cursor;
use warpui::{AppContext, Element, EntityId, SingletonEntity};

use crate::ai::agent::conversation::{AIConversationId, ConversationStatus};
use crate::ai::blocklist::block::view_impl::output::format_compact_elapsed;
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::appearance::Appearance;
use crate::ui_components::blended_colors;
use crate::workspace::WorkspaceAction;

const MAX_VISIBLE_ROWS: usize = 6;
const MAX_TITLE_CHARS: usize = 18;
const LIST_WIDTH: f32 = 306.;

#[derive(Clone)]
pub(super) struct AgentPresenceEntry {
    conversation_id: AIConversationId,
    terminal_view_id: EntityId,
    status: ConversationStatus,
    title: String,
    elapsed: Option<String>,
}

pub(super) struct AgentPresenceSummary {
    working: usize,
    blocked: usize,
    failed: usize,
    /// 按 `阻塞 > 失败 > 运行中` 排序后的会话列表。
    entries: Vec<AgentPresenceEntry>,
}

impl AgentPresenceSummary {
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn blocked_count(&self) -> usize {
        self.blocked
    }
}

fn status_priority(status: &ConversationStatus) -> u8 {
    match status {
        ConversationStatus::Blocked { .. } => 3,
        ConversationStatus::Error => 2,
        ConversationStatus::InProgress => 1,
        ConversationStatus::Success | ConversationStatus::Cancelled => 0,
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    let mut short: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        short.push('…');
    }
    short
}

/// 扫描所有活跃会话，汇总当前 Agent 状态。
///
/// 子 Agent 会话不单独计数，避免主会话的内部分支把数字撑大。
pub(super) fn summarize(app: &AppContext) -> AgentPresenceSummary {
    let history = BlocklistAIHistoryModel::as_ref(app);
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for (terminal_view_id, conversation) in history.all_live_conversations() {
        if !seen.insert(conversation.id()) {
            continue;
        }
        if conversation.parent_conversation_id().is_some() {
            continue;
        }
        if history.is_terminal_view_conversation_transcript_viewer(terminal_view_id) {
            continue;
        }

        let status = conversation.status();
        if status_priority(status) == 0 {
            continue;
        }

        let title = conversation
            .title()
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let title = if title.is_empty() {
            "未命名会话".to_owned()
        } else {
            truncate_chars(&title, MAX_TITLE_CHARS)
        };

        let elapsed = conversation.latest_exchange().and_then(|exchange| {
            let end = exchange.finish_time.unwrap_or_else(Local::now);
            end.signed_duration_since(exchange.start_time)
                .to_std()
                .ok()
                .map(format_compact_elapsed)
        });

        entries.push(AgentPresenceEntry {
            conversation_id: conversation.id(),
            terminal_view_id,
            status: status.clone(),
            title,
            elapsed,
        });
    }

    entries.sort_by(|a, b| {
        status_priority(&b.status)
            .cmp(&status_priority(&a.status))
            .then_with(|| a.title.cmp(&b.title))
    });

    let working = entries
        .iter()
        .filter(|entry| entry.status == ConversationStatus::InProgress)
        .count();
    let blocked = entries
        .iter()
        .filter(|entry| entry.status.is_blocked())
        .count();
    let failed = entries
        .iter()
        .filter(|entry| entry.status == ConversationStatus::Error)
        .count();

    AgentPresenceSummary {
        working,
        blocked,
        failed,
        entries,
    }
}

/// 渲染状态胶囊；没有需要关注的状态时返回 `None`，避免常驻噪音。
pub(super) fn render(
    summary: &AgentPresenceSummary,
    mouse_state: MouseStateHandle,
    list_mouse_state: MouseStateHandle,
    row_mouse_states: &RefCell<HashMap<AIConversationId, MouseStateHandle>>,
    keep_list_open: bool,
    appearance: &Appearance,
) -> Option<Box<dyn Element>> {
    if summary.is_empty() {
        return None;
    }
    let theme = appearance.theme();
    let font_family = appearance.ui_font_family();
    let font_size = appearance.ui_font_size() - 1.;

    let entries = [
        (summary.working, ConversationStatus::InProgress),
        (
            summary.blocked,
            ConversationStatus::Blocked {
                blocked_action: String::new(),
            },
        ),
        (summary.failed, ConversationStatus::Error),
    ];

    let mut rows = Vec::new();
    {
        let mut handles = row_mouse_states.borrow_mut();
        for entry in &summary.entries {
            let handle = handles.entry(entry.conversation_id).or_default().clone();
            rows.push((entry.clone(), handle));
        }
    }

    let most_urgent = rows
        .first()
        .map(|(entry, _)| (entry.conversation_id, entry.terminal_view_id));

    let pill = Hoverable::new(mouse_state, move |state| {
        let is_hovered = state.is_hovered();
        let background = if is_hovered {
            theme.surface_3()
        } else {
            theme.surface_2()
        };

        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(10.);

        for (count, status) in entries.iter() {
            if *count == 0 {
                continue;
            }
            let (icon, color) = status.status_icon_and_color(theme);
            row = row.with_child(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(3.)
                    .with_child(
                        ConstrainedBox::new(icon.to_warpui_icon(color.into()).finish())
                            .with_width(12.)
                            .with_height(12.)
                            .finish(),
                    )
                    .with_child(
                        Text::new(count.to_string(), font_family.clone(), font_size)
                            .with_color(color)
                            .with_selectable(false)
                            .finish(),
                    )
                    .finish(),
            );
        }

        let content = Container::new(row.finish())
            .with_background(background)
            .with_border(Border::all(1.).with_border_fill(blended_colors::neutral_2(theme)))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .with_padding_left(8.)
            .with_padding_right(8.)
            .with_padding_top(3.)
            .with_padding_bottom(3.)
            .finish();

        if !is_hovered && !keep_list_open {
            return content;
        }

        let mut list = Flex::column().with_spacing(2.);
        for (entry, handle) in rows.iter().take(MAX_VISIBLE_ROWS) {
            let conversation_id = entry.conversation_id;
            let terminal_view_id = entry.terminal_view_id;
            let title = entry.title.clone();
            let (right_text, is_action) = match &entry.status {
                ConversationStatus::Blocked { .. } => (Some("去确认".to_owned()), true),
                ConversationStatus::Error => (Some("查看".to_owned()), true),
                _ => (entry.elapsed.clone(), false),
            };
            let (icon, color) = entry.status.status_icon_and_color(theme);

            let list_row = Hoverable::new(handle.clone(), move |state| {
                let background = if state.is_hovered() {
                    theme.surface_2()
                } else {
                    theme.surface_3()
                };
                let text_color = blended_colors::text_main(theme, theme.surface_3());
                let muted_color = blended_colors::text_disabled(theme, theme.surface_3());

                let mut row = Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.)
                    .with_child(
                        ConstrainedBox::new(icon.to_warpui_icon(color.into()).finish())
                            .with_width(12.)
                            .with_height(12.)
                            .finish(),
                    )
                    .with_child(
                        ConstrainedBox::new(
                            Text::new(title.clone(), font_family.clone(), font_size)
                                .with_color(text_color)
                                .with_selectable(false)
                                .finish(),
                        )
                        .with_width(210.)
                        .finish(),
                    );
                if let Some(right_text) = right_text.clone() {
                    row = row.with_child(
                        Text::new(right_text, font_family.clone(), font_size)
                            .with_color(if is_action { color } else { muted_color })
                            .with_selectable(false)
                            .finish(),
                    );
                }

                Container::new(row.finish())
                    .with_background(background)
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                    .with_horizontal_padding(6.)
                    .with_vertical_padding(4.)
                    .finish()
            })
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(WorkspaceAction::SetAgentPresenceListHovered(false));
                ctx.dispatch_typed_action(WorkspaceAction::RestoreOrNavigateToConversation {
                    pane_view_locator: None,
                    window_id: None,
                    conversation_id,
                    terminal_view_id: Some(terminal_view_id),
                    restore_layout: None,
                });
            });

            list = list.with_child(list_row.finish());
        }

        if summary.entries.len() > MAX_VISIBLE_ROWS {
            list = list.with_child(
                Text::new(
                    format!("还有 {} 个会话", summary.entries.len() - MAX_VISIBLE_ROWS),
                    font_family.clone(),
                    font_size,
                )
                .with_color(blended_colors::text_disabled(theme, theme.surface_3()))
                .with_selectable(false)
                .finish(),
            );
        }

        let list = ConstrainedBox::new(
            Container::new(list.finish())
                .with_background(theme.surface_3())
                .with_border(Border::all(1.).with_border_fill(theme.outline()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
                .with_uniform_padding(6.)
                .finish(),
        )
        .with_width(LIST_WIDTH)
        .finish();

        let list = Hoverable::new(list_mouse_state.clone(), move |_state| list)
            .on_hover(move |hovered, ctx, _, _| {
                ctx.dispatch_typed_action(WorkspaceAction::SetAgentPresenceListHovered(hovered));
            })
            .finish();

        let mut stack = Stack::new().with_child(content);
        stack.add_positioned_overlay_child(
            list,
            OffsetPositioning::offset_from_parent(
                vec2f(0., 6.),
                ParentOffsetBounds::WindowByPosition,
                ParentAnchor::BottomRight,
                ChildAnchor::TopRight,
            ),
        );
        stack.finish()
    })
    .with_hover_out_delay(Duration::from_millis(200))
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        let Some((conversation_id, terminal_view_id)) = most_urgent else {
            return;
        };
        ctx.dispatch_typed_action(WorkspaceAction::SetAgentPresenceListHovered(false));
        ctx.dispatch_typed_action(WorkspaceAction::RestoreOrNavigateToConversation {
            pane_view_locator: None,
            window_id: None,
            conversation_id,
            terminal_view_id: Some(terminal_view_id),
            restore_layout: None,
        });
    });

    Some(pill.finish())
}
