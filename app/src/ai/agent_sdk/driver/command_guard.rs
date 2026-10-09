//! AI 下发 shell 命令的客户端强制规则。
//!
//! # 为什么要客户端强制，而不是只写提示词
//!
//! 提示词里的“禁止运行 `git push`”属于**软约束**：LLM 的指令遵循是概率性的，
//! 长上下文 / 多轮对话 / 任务压力大时被违反是常态而非例外。把硬性要求寄托在
//! 提示词上，失效是必然的，只是时间问题。
//!
//! 本模块提供的是**硬约束**：命令在下发到 shell 之前先经过
//! [`check_command`]，不合规的压根不会进入终端会话。
//!
//! # 为什么会卡死
//!
//! 交互式命令（`ssh`、`git push`、需要密码的 `curl` 等）被下发后，
//! `TerminalView::execute_command_or_set_pending` 会把它设为 pending，
//! 等 `BlockCompleted` / `BootstrapPrecmdDone` 事件才真正执行。
//! 而交互式命令在等人工输入，block 永远不会完成 —— AI 等 block、
//! shell 等输入，互相死等，表现为“一直等待用户输入”。
//!
//! 配合 [`super::terminal`] 里的超时兜底，可以同时挡住：
//! - 本模块能识别的已知危险命令（快速、确定、可解释）
//! - 以及所有本模块没预料到的未知挂起情况（超时兜底）

use std::time::Duration;

/// `wait_until_complete=true` 时命令的最长等待时间。
///
/// 超时后会关闭 PTY 并把等待方唤醒，把“疑似卡在交互状态”回传给模型。
///
/// 取 30s 而不是更长：卡住的命令不会自己恢复，等待越久体验越差；
/// 而正常的构建 / 测试命令通常远快于这个时间。误杀长任务时，
/// 模型会看到明确的超时错误并自行改用 `wait_until_complete=false` 重跑。
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// 一条命令被客户端拒绝时的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    /// 需要交互输入（密码、确认、TTY），在 AI 工具里必然卡死。
    Interactive { command: String },
    /// 需要 TTY 的全屏程序。
    NeedsTty { command: String },
    /// 已知会把终端挂起、无法自行退出的程序。
    NeverExits { command: String },
    /// heredoc（`<<'PY'`）——向终端灌多行文本，极易因尾部多余字符而未闭合。
    ///
    /// 未闭合时 shell 会切到 PS2 续行提示符（`>`）静默等待输入，表现为“卡死”。
    Heredoc { command: String },
    /// 引号 / 括号 / `$(` 未配平，同样会把 shell 留在续行状态。
    Unbalanced { command: String, detail: String },
    /// 未闭合反引号 `` ` ``（PTY 实测会进入续行）。
    Backtick { command: String },
    /// 命令以管道 / 逻辑运算符 / 反斜杠结尾，右侧缺命令（PTY 实测会进入续行）。
    DanglingOperator { command: String, op: String },
    /// 会从 stdin 读数据但拿不到输入的命令（`cat`、`read` 等）。
    ReadsStdin { command: String },
}

impl RejectionReason {
    /// 面向模型的可执行建议。
    ///
    /// 关键点：不只是说“禁止”，而是告诉模型**该怎么做**，否则它多半会换个
    /// 写法再撞一次。
    pub fn model_hint(&self) -> String {
        match self {
            RejectionReason::Interactive { .. } => {
                "该命令需要交互输入（如密码 / 确认 / TTY），在 Agent 工具中运行会卡住直到超时。\n\
                 正确做法：\n\
                 - 如果只是需要设置环境变量，用 run_shell_command 的 env 前缀或拆成多条命令，不要与命令用 `;` 组合。\n\
                 - 如果确实需要认证，告诉用户“请手动在终端运行：<command>”，让用户自己执行，不要代劳。\n\
                 - 如果只是想查看状态，用非交互式替代（如 `git status` 代替 `git push --dry-run`）。"
                    .to_owned()
            }
            RejectionReason::NeedsTty { .. } => {
                "该命令需要 TTY（全屏交互界面），无法在 Agent 工具中运行。\n\
                 请让用户手动在终端中执行。"
                    .to_owned()
            }
RejectionReason::NeverExits { .. } => {
                "该命令不会自行退出，会导致本轮一直等待。\n\
                 正确做法：为 run_shell_command 传入 wait_until_complete=false，\
                 拿到 command_id 后用 read_shell_command_output 读取输出、\
                 用 write_to_long_running_shell_command 发送 Ctrl-C 终止。\n\
                 如果只是想看看文件开头，请改用 read_files 工具。"
                    .to_owned()
            }
            RejectionReason::Heredoc { .. } => {
                "不允许使用 heredoc（`<<'PY'` / `<<EOF`）向终端注入多行文本。\n\
                 heredoc 一旦因尾部多一个字符（如多出的 `)`）而未闭合，shell 会切到续行提示符（`>`）\
                 静默等待输入，本轮就会卡死。\n\
                 正确做法：\n\
                 - 用 apply_file_diffs 把脚本写入文件，再用 `python3 script.py` 执行。\n\
                 - 单行脚本改用 `python3 -c \"...\"`（注意双引号内的引号转义）。\n\
                 - 管道写法 `printf \"...\" | python3` 同样可行。"
                    .to_owned()
            }
RejectionReason::Unbalanced { detail, .. } => {
                format!(
                    "命令的 {detail} 未配平，shell 会进入续行状态等待后续输入，导致本轮卡死。\n\
                     请修正后重试；如果确实需要多行内容，请改用 apply_file_diffs 写入文件再执行。"
                )
            }
            RejectionReason::Backtick { .. } => {
                "命令中的反引号 ` 未闭合，shell 会进入续行状态等待后续输入。\n\
                 反引号内不能包含未转义的反引号；请改用 `$( )` 或修正引号配平。"
                    .to_owned()
            }
            RejectionReason::DanglingOperator { op, .. } => {
                format!(
                    "命令以 `{op}` 结尾，右侧缺少要执行的命令，shell 会进入续行状态永久等待。\n\
                     请补全该运算符右侧的命令（例如把管道写成一行：`a | b`），\n\
                     或改用 `&&` 连接两条完整命令。"
                )
            }
            RejectionReason::ReadsStdin { .. } => {
                "该命令会从 stdin 读取数据，但在 Agent 工具中拿不到输入，会一直等待。\n\
                 正确做法：\n\
                 - 查看文件内容请用 read_files 工具，而不是 `cat`。\n\
                 - 需要管道输入请写成一行，例如 `echo foo | cat`。\n\
                 - 如果确实需要交互输入，请告诉用户手动在终端执行。"
                    .to_owned()
            }
        }
    }
}

/// 简单判定：命令的第一个词（去掉路径和环境变量前缀）是否命中黑名单。
///
/// 只看**第一个词**，因为语义由它决定：`git push` 的危险来自 `push`
/// 而不是 `git` 本身（`git status` 是安全的）。
fn first_word(command: &str) -> String {
    // 去掉 `FOO=bar` 前缀和 `sudo` 之类包装，取到真正的可执行文件名。
    let mut rest = command.trim_start();
    loop {
        let before = rest.len();
        for prefix in ["sudo ", "env ", "command ", "nohup ", "time ", "xargs "] {
            if let Some(stripped) = rest.strip_prefix(prefix) {
                rest = stripped.trim_start();
            }
        }
        // `FOO=bar cmd` / `FOO= cmd` 形式。注意要允许 `=` 和引号，否则
        // `PAGER=cat less x` 这种最常见的写法会被漏掉。
        if let Some(idx) = rest.find(' ') {
            let head = &rest[..idx];
            let is_env_assignment = !head.is_empty()
                && head
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_uppercase() || b == b'_')
                && head.bytes().all(|b| {
                    b.is_ascii_alphanumeric()
                        || b == b'_'
                        || b == b'='
                        || b == b'.'
                        || b == b'-'
                        || b == b'"'
                        || b == b'\''
                });
            if is_env_assignment {
                rest = rest[idx..].trim_start();
            }
        }
        if rest.len() == before {
            break;
        }
    }
    // 取 basename，去掉路径
    let token = rest.split_whitespace().next().unwrap_or("");
    let base = token.rsplit('/').next().unwrap_or(token);
    base.trim_matches(|c| c == '(' || c == ')' || c == '\\')
        .to_owned()
}

/// `git` 的子命令里，哪些是交互式 / 会触发认证的。
///
/// `git status` / `git log` / `git diff` 等是安全的读操作，不在此列。
const GIT_BLOCKED_SUBCOMMANDS: &[&str] = &[
    "push",
    "pull",
    "clone",
    "fetch",
    "remote",
    "submodule",
    "credential",
    "lfs",
];

/// 需要交互输入的命令。
const INTERACTIVE: &[&str] = &[
    "ssh",
    "scp",
    "sftp",
    "ftp",
    "telnet",
    "mysql",
    "psql",
    "mongo",
    "redis-cli",
    "sqlite3",
    "gpg",
    "pass",
    "sudo",
    "su",
    "login",
    "fdisk",
    "parted",
    "ssh-add",
    "ssh-keygen",
    "gdb",
    "docker",
    "kubectl",
    "helm",
    "vagrant",
    "terraform",
    "rclone",
    "gh",
    "az",
    "aws",
];

/// 需要 TTY 的全屏程序。
const NEEDS_TTY: &[&str] = &[
    "vim", "vi", "nvim", "emacs", "nano", "pico", "ed", "helix", "hx", "micro", "kak", "less",
    "more", "man", "top", "htop", "btop", "tmux", "screen", "watch", "watchman", "dialog",
    "whiptail", "fzf", "ranger", "ncdu", "tig",
];

/// 不会自行退出的程序（除非配合 timeout）。
const NEVER_EXITS: &[&str] = &[
    "tail",
    "journalctl",
    "yes",
    "sleep",
    "ping",
    "nc",
    "netcat",
    "socat",
    "telnet",
];

/// 检查命令是否应该被客户端拒绝。
///
/// 返回 `Ok(())` 表示放行，`Err(reason)` 表示拒绝并携带可回传给模型的原因。
///
/// 这一层刻意**只拦确定会卡死的模式**——语义等价但写法更复杂、可能只是
/// 看起来危险的命令一律放行，避免误伤正常命令。兜底交给
/// [`COMMAND_TIMEOUT`]。
pub fn check_command(command: &str) -> Result<(), RejectionReason> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    // heredoc：单独拎出来先查，因为它是实践中最常见的卡死原因。
    if find_heredoc(trimmed).is_some() {
        return Err(RejectionReason::Heredoc {
            command: trimmed.to_owned(),
        });
    }

    // 续行类：反引号未闭合 / 行尾悬空运算符。
    if let Some(detail) = continuation_hazard(trimmed) {
        return Err(detail);
    }

    // 未配平引号（PTY 实测会进入 `>` 续行）。
    if let Some(detail) = unbalanced_reason(trimmed) {
        return Err(RejectionReason::Unbalanced {
            command: trimmed.to_owned(),
            detail,
        });
    }

    // `A; B` 组合里的危险在第二段，光看开头会漏。
    // `&&` / `||` / `;` / `|` 分隔的每一段都要看——
    // 这正是 `export FOO=bar; interactive_cmd` 这类写法的藏身处。
    for (segment, sep_before) in split_segments(trimmed) {
        // 管道右侧的段不吃 stdin：`echo x | cat` 里的 cat 拿得到输入。
        let is_pipe_right_side = sep_before == Some('|');
        check_single(segment.trim(), trimmed, is_pipe_right_side)?;
    }
    Ok(())
}

/// 按 shell 控制运算符切分命令，忽略引号内的运算符。
///
/// 返回 `(段内容, 该段前面的分隔符)`。分隔符用于判断管道右侧
/// （管道右侧的命令不读 stdin）。
fn split_segments(command: &str) -> Vec<(&str, Option<char>)> {
    let mut segments = Vec::new();
    let bytes = command.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    let mut sep_before: Option<char> = None;

    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'\'' | b'"' => quote = Some(b),
                b';' | b'|' | b'&' | b'\n' => {
                    segments.push((&command[start..i], sep_before));
                    sep_before = Some(b as char);
                    start = i + 1;
                    // `&&` / `||` 跳过第二个字符
                    if matches!(b, b'&' | b'|') && bytes.get(i + 1) == Some(&b) {
                        i += 1;
                        start = i + 1;
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    segments.push((&command[start..], sep_before));
    segments
        .into_iter()
        .filter(|(s, _)| !s.trim().is_empty())
        .collect()
}

/// 检查单条命令（不含控制运算符）。`original` 用于错误信息。
fn check_single(
    segment: &str,
    original: &str,
    is_pipe_right_side: bool,
) -> Result<(), RejectionReason> {
    if segment.is_empty() {
        return Ok(());
    }

    let head = first_word(segment);

    if NEEDS_TTY.contains(&head.as_str()) {
        return Err(RejectionReason::NeedsTty {
            command: original.to_owned(),
        });
    }
    if INTERACTIVE.contains(&head.as_str()) {
        return Err(RejectionReason::Interactive {
            command: original.to_owned(),
        });
    }

    // `git` 单独看第一词不够，要看子命令。
    if head == "git" {
        if let Some(sub) = git_subcommand(segment) {
            if GIT_BLOCKED_SUBCOMMANDS.contains(&sub.as_str()) {
                return Err(RejectionReason::Interactive {
                    command: original.to_owned(),
                });
            }
        }
        return Ok(());
    }

    // 不会自行退出的命令：只在 `wait_until_complete=true` 时才是问题，
    // 这里统一拦掉，提示模型改用长运行模式。
    if is_never_exits(segment, &head) {
        return Err(RejectionReason::NeverExits {
            command: original.to_owned(),
        });
    }

    // 裸 `cat` / `read` 会永久等 stdin。管道右侧的段不吃 stdin，故不判。
    if !is_pipe_right_side && reads_stdin(segment) {
        return Err(RejectionReason::ReadsStdin {
            command: original.to_owned(),
        });
    }

    // 未配平的引号 / 括号会让 shell 留在续行状态。
    if let Some(detail) = unbalanced_reason(original) {
        return Err(RejectionReason::Unbalanced {
            command: original.to_owned(),
            detail,
        });
    }

    Ok(())
}

/// 检测两类会让 shell 进入续行状态的写法。
///
/// 均为 PTY 实测确认（`pty.fork()` + 交互式 bash）：
/// - `echo \`date`   → `>` 续行，永久等待
/// - `cat f |` / `ls &&` / `false ||` / `echo a \` → `>` 续行，永久等待
///
/// **只判“字符串末尾”**：多行管道（`echo hi |\nwc -l`）是合法语法，
/// PTY 实测能正常执行，所以不能见到 `|` 就拦。
fn continuation_hazard(command: &str) -> Option<RejectionReason> {
    // 1) 反引号配平（反引号内可以嵌套，但不能包含未转义的反引号）
    if !backticks_balanced(command) {
        return Some(RejectionReason::Backtick {
            command: command.to_owned(),
        });
    }

    // 2) 行尾悬空运算符
    let trimmed_end = command.trim_end();
    if let Some(op) = dangling_operator(trimmed_end) {
        return Some(RejectionReason::DanglingOperator {
            command: command.to_owned(),
            op,
        });
    }

    None
}

/// 反引号是否配对。简单计数即可：反引号内不能出现未转义的反引号。
fn backticks_balanced(command: &str) -> bool {
    let bytes = command.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'`' => count += 1,
            _ => {}
        }
        i += 1;
    }
    count % 2 == 0
}

/// 返回行尾的悬空运算符（如 `|` / `&&` / `||` / `\`），没有则 `None`。
fn dangling_operator(command: &str) -> Option<String> {
    let trimmed = command.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    // 行尾单个反斜杠（`\\`）：PTY 实测进入续行。
    // 注意区分以 `\\\\` （转义后的反斜杠）结尾：末尾连续反斜杠数量是奇数才是悬空。
    let trailing_backslashes = trimmed.bytes().rev().take_while(|&b| b == b'\\').count();
    if trailing_backslashes % 2 == 1 {
        return Some("\\".to_owned());
    }
    for op in ["&&", "||", "|"] {
        if trimmed.ends_with(op) {
            return Some(op.to_owned());
        }
    }
    None
}

/// 检测会从 stdin 读数据的命令。
///
/// 保守策略：**只拦最确定的少数几个**。PTY 实测 `cat`（无参数）与
/// `read x` 会永久等 stdin；其他命令即使可能读 stdin，风险收益比不划算，
/// 且误伤代价高。`cat file.txt`、`echo x | cat` 这类有输入的均放行。
fn reads_stdin(command: &str) -> bool {
    let head = first_word(command);
    let rest: Vec<&str> = command.split_whitespace().skip(1).collect();

    match head.as_str() {
        // `read` 永远从 stdin 读，不管有没有参数
        "read" => !rest.is_empty(),
        // `cat` 无参数 / 只有选项时才读 stdin
        "cat" => rest.iter().all(|t| t.starts_with('-')),
        _ => false,
    }
}
/// 检测 heredoc：`<<'DELIM'` / `<<-DELIM` / `<<DELIM`。
///
/// 返回终止符名（如果有）。heredoc 必须跨行注入，而 Zap 是逐行往 PTY 写的，
/// 极易因尾部多一个字符而未闭合，shell 会切到 PS2 静默等输入。
fn find_heredoc(command: &str) -> Option<String> {
    let bytes = command.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'\'' | b'"' => quote = Some(b),
                b'\\' => i += 1,
                b'<' if bytes[i + 1] == b'<' => {
                    let mut j = i + 2;
                    // 跳过 `<<-` 的短横线
                    if j < bytes.len() && bytes[j] == b'-' {
                        j += 1;
                    }
                    // 跳过定界符的引号与转义
                    if j < bytes.len() && matches!(bytes[j], b'\'' | b'"') {
                        j += 1;
                    }
                    let start = j;
                    while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_')
                    {
                        j += 1;
                    }
                    let delim = command[start..j].to_owned();
                    if !delim.is_empty() {
                        return Some(delim);
                    }
                    i = j;
                    continue;
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// 检查引号 / 括号是否配平。
///
/// 只做**保守判定**：能确定未配平时才返回原因；拿不准的一律视为配平，
/// 避免误伤正常命令。
///
/// 真实 PTY 实测结论（决定了这里的保守程度）：
/// - `echo 'foo` → shell 进入 `>` 续行，永久等待 → 必须拦
/// - `echo (1))` → bash 报语法错误后退出，不会挂 → 拦了只是提前报错
/// - `python3 -c "print(1))"` → 程序报错退出，不挂 → 同上
/// 因此**只拦未闭合引号**（真会挂），不拦括号不配平（只会提前报错）。
/// 宁可漏放也不能误伤：误伤会让正常命令无故失败。
fn unbalanced_reason(command: &str) -> Option<String> {
    let bytes = command.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                // shell 里反斜杠在单引号内**不**转义，双引号内才转义。
                if b == b'\\' && q == b'"' && i + 1 < bytes.len() {
                    // 双引号内的转义：跳过下一个字节
                    i += 2;
                    continue;
                }
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'\'' | b'"' => quote = Some(b),
                b'\\' if i + 1 < bytes.len() => {
                    // 引号外可能是转义（如 `\"`），跳过下一个字节
                    i += 2;
                    continue;
                }
                b'$' if bytes.get(i + 1) == Some(&b'(') => {
                    // `$(`：跳过整个命令替换内容，避免里面的括号/引号干扰
                    if let Some(end) = find_matching_paren(bytes, i + 1) {
                        i = end + 1;
                        continue;
                    } else {
                        // 未闭合的命令替换会让 shell 进入续行（PTY 实测）
                        return Some("命令替换 `$(`".to_owned());
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }

    quote.map(|q| {
        if q == b'\'' {
            "单引号".to_owned()
        } else {
            "双引号".to_owned()
        }
    })
}

/// 从 `open_idx`（指向 `(`）开始，找到配对的 `)`。找不到返回 `None`。
fn find_matching_paren(bytes: &[u8], open_idx: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    for i in open_idx..bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'\'' | b'"' => quote = Some(b),
                b'\\' => {}
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            },
        }
    }
    None
}

/// 取出 `git` 的子命令，跳过全局选项及其参数（`-C dir`、`--git-dir=...` 等）。
fn git_subcommand(command: &str) -> Option<String> {
    // 全局选项里，哪些会额外吃掉一个参数。
    const OPTIONS_WITH_ARG: &[&str] = &[
        "-C",
        "-c",
        "--git-dir",
        "--work-tree",
        "--namespace",
        "--exec-path",
    ];

    let mut tokens = command.split_whitespace().skip(1);
    while let Some(token) = tokens.next() {
        if !token.starts_with('-') {
            return Some(token.to_owned());
        }
        // `-C /tmp`、`-c key=val`：跳过紧随的参数，否则会把 `/tmp` 误认成子命令。
        if OPTIONS_WITH_ARG.contains(&token) {
            tokens.next();
        }
        // `--git-dir=/x` 形式自带值，无需额外跳过。
    }
    None
}

/// 判断命令是否属于「不会自行退出」类。
///
/// 除了黑名单前缀，也匹配「命令 + 跟随参数」的形式（如 `tail -f`），
/// 因为这类命令即使有 `-f` 也同样是挂起源。
fn is_never_exits(command: &str, head: &str) -> bool {
    if NEVER_EXITS.iter().any(|p| *p == head) {
        return true;
    }
    // `X -f` / `X --follow` 形式的跟随参数
    if matches!(head, "tail" | "journalctl" | "grep" | "sed") && command.contains(" -f") {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason_of(cmd: &str) -> Option<RejectionReason> {
        check_command(cmd).err()
    }

    #[test]
    fn blocks_interactive_commands() {
        assert!(matches!(
            reason_of("ssh user@host"),
            Some(RejectionReason::Interactive { .. })
        ));
        assert!(matches!(
            reason_of("mysql -u root"),
            Some(RejectionReason::Interactive { .. })
        ));
    }

    #[test]
    fn blocks_tty_programs() {
        assert!(matches!(
            reason_of("vim file.txt"),
            Some(RejectionReason::NeedsTty { .. })
        ));
        assert!(matches!(
            reason_of("top"),
            Some(RejectionReason::NeedsTty { .. })
        ));
    }

    #[test]
    fn blocks_never_exits() {
        assert!(matches!(
            reason_of("tail -f log.txt"),
            Some(RejectionReason::NeverExits { .. })
        ));
        assert!(matches!(
            reason_of("tail log.txt"),
            Some(RejectionReason::NeverExits { .. })
        ));
    }

    #[test]
    fn blocks_git_write_subcommands_but_allows_read_only() {
        // 会触发认证/远端交互的子命令应拦
        for cmd in [
            "git push",
            "git pull",
            "git clone https://github.com/u/r.git",
            "git fetch origin",
            "git remote set-url origin https://x",
            "git lfs install",
        ] {
            assert!(reason_of(cmd).is_some(), "`{cmd}` 应当被拒绝，但它通过了");
        }
        // 只读子命令必须放行，否则会严重误伤
        for cmd in [
            "git status",
            "git log",
            "git diff",
            "git add .",
            "git commit -m 'x'",
            "git -C /tmp/repo status",
            "git --git-dir=/tmp/x status",
        ] {
            assert!(reason_of(cmd).is_none(), "`{cmd}` 不应被拒绝，但它被拒了");
        }
    }

    #[test]
    fn strips_env_and_sudo_prefixes() {
        assert!(matches!(
            reason_of("sudo vim x"),
            Some(RejectionReason::NeedsTty { .. })
        ));
        assert!(matches!(
            reason_of("PAGER=cat less file"),
            Some(RejectionReason::NeedsTty { .. })
        ));
        assert!(matches!(
            reason_of("export PAGER=cat; less file"),
            Some(RejectionReason::NeedsTty { .. })
        ));
        assert!(matches!(
            reason_of("/usr/bin/ssh host"),
            Some(RejectionReason::Interactive { .. })
        ));
    }

    #[test]
    fn allows_ordinary_commands() {
        for cmd in [
            "ls -la",
            "cargo test",
            "npm install",
            "make build",
            "cat file.txt",
            "",
            "   ",
            "echo hello",
            "grep -r foo .",
        ] {
            assert!(reason_of(cmd).is_none(), "`{cmd}` 不应被拒绝，但它被拒了");
        }
    }

    #[test]
    fn git_subcommand_skips_global_options() {
        assert_eq!(
            git_subcommand("git -C /tmp status"),
            Some("status".to_owned())
        );
        assert_eq!(
            git_subcommand("git push origin main"),
            Some("push".to_owned())
        );
        assert_eq!(
            git_subcommand("git --git-dir=/tmp/x push"),
            Some("push".to_owned())
        );
    }

    #[test]
    fn blocks_heredoc() {
        // 用户实际遇到的场景：heredoc 尾部多一个 `)`，shell 留在 `>` 续行状态。
        for cmd in [
            "python3 - <<'PY'\nprint(1)\nPY",
            "python3 - <<EOF\necho hi\nEOF",
            "python3 - <<-'PY'\nprint(1)\nPY",
            "node - <<'JS'\nconsole.log(1)\nJS",
        ] {
            assert!(
                matches!(check_command(cmd), Err(RejectionReason::Heredoc { .. })),
                "`{cmd}` 应当被识别为 heredoc 而拒绝，但它通过了"
            );
        }
    }

    #[test]
    fn blocks_unclosed_quotes_and_subshells() {
        // PTY 实测确认：这两种会让 shell 进入 `>` 续行并永久等待。
        for cmd in [
            "echo 'unclosed",
            "echo \"unclosed",
            "echo $(ls",
            "python3 -c \"print(1)",
        ] {
            assert!(
                matches!(check_command(cmd), Err(RejectionReason::Unbalanced { .. })),
                "`{cmd}` 应当被判为未闭合，但它通过了"
            );
        }
    }

    #[test]
    fn allows_unbalanced_parentheses_because_shell_exits() {
        // PTY 实测：`echo (1))` / `python3 -c \"print(1))\"` 只会报错退出，不会挂。
        // 因此不拦它们 —— 误伤正常命令的代价高于漏放一个会快速失败的命令。
        for cmd in [
            "echo (1))",
            "python3 -c \"print(1))\"",
            "echo \"a(b)c\"",
            "awk '{print $1}' file.txt",
            "echo \"it's\"",
            "echo 'nested $(echo $(echo hi))'",
            // 双引号内的单引号是字面量，引号实际是配平的（PTY 实测不会挂）
            "python3 -c \"print('hi')\"",
            "echo \"don't\"",
        ] {
            assert!(check_command(cmd).is_ok(), "`{cmd}` 不应被拒绝，但它被拒了");
        }
    }

    #[test]
    fn allows_balanced_complex_commands() {
        // 正常的多层嵌套不能被误伤
        for cmd in [
            "python3 -c \"print('hi')\"",
            "echo \"a(b)c\"",
            "ls $(pwd)",
            "echo \"$(date)\"",
            "node -e \"console.log(require('fs').readdirSync('.'))\"",
            "git commit -m \"fix: 修复反斜杠 \\\\ 和引号 \\\"\"",
        ] {
            assert!(check_command(cmd).is_ok(), "`{cmd}` 不应被拒绝，但它被拒了");
        }
    }

    #[test]
    fn blocks_backtick_and_dangling_operators() {
        // PTY 实测确认会让 shell 进入 `>` 续行的几种写法
        for cmd in ["echo `date", "cat f |", "ls &&", "false ||", "echo a \\"] {
            assert!(
                check_command(cmd).is_err(),
                "`{cmd}` 应当被拒绝，但它通过了"
            );
        }
    }

    #[test]
    fn allows_multiline_operators_that_are_legitimate() {
        // 关键：多行管道是合法 shell 语法（PTY 实测能正常执行），不能误伤。
        for cmd in [
            "echo hi |\nwc -l",
            "true &&\necho ok",
            "cat file.txt | grep foo | wc -l",
            "echo `date`",
            "echo `echo \\`date\\``",
        ] {
            assert!(
                check_command(cmd).is_ok(),
                "`{cmd}` 是合法语法，不应被拒绝，但它被拒了"
            );
        }
    }

    #[test]
    fn blocks_commands_that_read_stdin() {
        assert!(matches!(
            check_command("cat"),
            Err(RejectionReason::ReadsStdin { .. })
        ));
        assert!(matches!(
            check_command("read x"),
            Err(RejectionReason::ReadsStdin { .. })
        ));
    }

    #[test]
    fn allows_stdin_readers_when_input_is_available() {
        // 有输入的 cat / 管道右侧的 cat 都必须放行
        for cmd in [
            "cat file.txt",
            "echo x | cat",
            "cat < input.txt",
            "cat -n file.txt",
            "head -20 file.txt",
        ] {
            assert!(check_command(cmd).is_ok(), "`{cmd}` 不应被拒绝，但它被拒了");
        }
    }

    #[test]
    fn model_hint_is_actionable() {
        // 拒绝必须带可执行建议，否则模型会换个写法再撞一次
        for reason in [
            RejectionReason::Interactive {
                command: "git push".to_owned(),
            },
            RejectionReason::NeedsTty {
                command: "vim x".to_owned(),
            },
            RejectionReason::NeverExits {
                command: "tail -f x".to_owned(),
            },
            RejectionReason::Heredoc {
                command: "python3 - <<'PY'".to_owned(),
            },
            RejectionReason::Unbalanced {
                command: "echo 'x".to_owned(),
                detail: "单引号".to_owned(),
            },
            RejectionReason::Backtick {
                command: "echo `x".to_owned(),
            },
            RejectionReason::DanglingOperator {
                command: "cat |".to_owned(),
                op: "|".to_owned(),
            },
            RejectionReason::ReadsStdin {
                command: "cat".to_owned(),
            },
        ] {
            let hint = reason.model_hint();
            assert!(hint.len() > 40, "提示过短，可能无法指导模型纠正：{hint}");
            assert!(
                hint.contains("手动")
                    || hint.contains("wait_until_complete")
                    || hint.contains("apply_file_diffs")
                    || hint.contains("重试")
                    || hint.contains("补全")
                    || hint.contains("修正"),
                "提示应给出可执行出路：{hint}"
            );
        }
    }

    /// 与真实 PTY 实测结果对照（`pty.fork()` + 交互式 bash）。
    ///
    /// 这张表是本次实现的依据：起初我以为“括号不配平”会挂，实测发现
    /// bash 只报语法错就退出；真正会挂的是未闭合引号、未闭合 `$(`、
    /// 以及**结尾悬空的控制运算符**。测试锁住这个认知，防止回退。
    #[test]
    fn matches_real_pty_behaviour() {
        let expectations = [
            ("echo 'unclosed", true),
            ("echo \"unclosed", true),
            ("echo $(", true),
            ("echo a |", true),
            ("echo a &&", true),
            ("echo a ||", true),
            ("echo a \\", true),
            ("python3 - <<'PY'", true),
            // PTY 实测**不会**挂的写法，不能误伤
            ("echo (1))", false),
            ("echo )))", false),
            ("echo []]", false),
            ("python3 -c 'print(1))'", false),
            ("echo $(pwd)", false),
            ("echo 'it'\\''s'", false),
            ("awk '{print $1}' f", false),
            ("if true; then echo x; fi", false),
            ("echo hello", false),
            ("git status", false),
        ];
        for (cmd, expect_blocked) in expectations {
            let blocked = check_command(cmd).is_err();
            assert_eq!(
                blocked, expect_blocked,
                "`{cmd}` 预期 blocked={expect_blocked}，实际={blocked}"
            );
        }
    }
}
