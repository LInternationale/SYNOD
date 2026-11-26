//! Interactive terminal client.
use super::{
    bulletin::{BoardAction, BoardRows, Bulletin, DraftView},
    preferences::{SettingsAction, ThemeColor, ThemeForm},
};
use crate::{
    application::setup::NewTeam,
    session::{bootstrap, protocol::*, transport::Client},
};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Position, Rect},
    widgets::{Paragraph, Wrap},
};
use std::{
    io::{self, Stdout},
    path::Path,
};
type Term = Terminal<CrosstermBackend<Stdout>>;
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        let _ = terminal::disable_raw_mode();
    }
}
fn input() -> io::Result<Event> {
    let e = event::read()?;
    if matches!(e,Event::Key(k) if k.code==KeyCode::Char('c')&&k.modifiers.contains(KeyModifiers::CONTROL))
    {
        return Err(io::Error::from(io::ErrorKind::Interrupted));
    }
    Ok(e)
}

struct Field<'a> {
    label: &'a str,
    value: String,
}
fn field(label: &str, value: impl Into<String>) -> Field<'_> {
    Field {
        label,
        value: value.into(),
    }
}
struct Form<'a> {
    title: &'a str,
    fields: Vec<Field<'a>>,
    buttons: &'a [&'a str],
    focus: usize,
}
enum Action {
    Back,
    Button(usize),
}
impl<'a> Form<'a> {
    fn new(title: &'a str, fields: Vec<Field<'a>>, buttons: &'a [&'a str]) -> Self {
        Self {
            title,
            fields,
            buttons,
            focus: 0,
        }
    }
    fn interact(&mut self, t: &mut Term, color: ThemeColor, message: &str) -> io::Result<Action> {
        let count = self.fields.len() + self.buttons.len();
        let mut bounds = Vec::new();
        loop {
            t.draw(|f| {
                let area = f.area();
                let outer = super::page(self.title);
                let inner = outer.inner(area);
                f.render_widget(outer, area);
                let [fields, bar, status] = Layout::vertical([
                    Constraint::Min(0),
                    Constraint::Length(1),
                    Constraint::Length(2),
                ])
                .areas(inner);
                bounds.clear();
                let visible = usize::from(fields.height).max(1);
                let first = self
                    .focus
                    .min(self.fields.len().saturating_sub(1))
                    .saturating_sub(visible - 1);
                let mut y = fields.y;
                for (i, field) in self.fields.iter().enumerate().skip(first) {
                    if y >= fields.bottom() {
                        break;
                    }
                    let height = if field.label == "正文" {
                        fields.bottom() - y
                    } else {
                        1
                    };
                    let rect = Rect::new(fields.x, y, fields.width, height);
                    let widget = Paragraph::new(format!("{}  {}", field.label, field.value))
                        .wrap(Wrap { trim: false });
                    f.render_widget(
                        if i == self.focus {
                            widget.style(color.highlight())
                        } else {
                            widget
                        },
                        rect,
                    );
                    bounds.push((i, rect));
                    y += height;
                }
                let zones = Layout::horizontal(vec![
                    Constraint::Ratio(1, self.buttons.len() as u32);
                    self.buttons.len()
                ])
                .split(bar);
                for (i, (label, rect)) in self.buttons.iter().zip(zones.iter()).enumerate() {
                    let idx = self.fields.len() + i;
                    let w = Paragraph::new(*label).centered();
                    f.render_widget(
                        if idx == self.focus {
                            w.style(color.highlight())
                        } else {
                            w
                        },
                        *rect,
                    );
                    bounds.push((idx, *rect));
                }
                f.render_widget(Paragraph::new(message).wrap(Wrap { trim: false }), status);
            })?;
            match input()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    KeyCode::Esc => return Ok(Action::Back),
                    code if let Some(next) = super::focus_step(code, self.focus, count) => {
                        self.focus = next
                    }
                    KeyCode::Enter if self.focus >= self.fields.len() => {
                        return Ok(Action::Button(self.focus - self.fields.len()));
                    }
                    KeyCode::Enter if self.fields[self.focus].label == "正文" => {
                        self.fields[self.focus].value.push('\n')
                    }
                    KeyCode::Enter => self.focus = (self.focus + 1) % count,
                    KeyCode::Backspace if self.focus < self.fields.len() => {
                        self.fields[self.focus].value.pop();
                    }
                    KeyCode::Char(c) if self.focus < self.fields.len() => {
                        self.fields[self.focus].value.push(c)
                    }
                    _ => {}
                },
                Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                    if let Some((idx, _)) = bounds
                        .iter()
                        .find(|(_, r)| r.contains(Position::new(m.column, m.row)))
                    {
                        self.focus = *idx;
                        if *idx >= self.fields.len() {
                            return Ok(Action::Button(*idx - self.fields.len()));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    fn val(&self, i: usize) -> String {
        self.fields[i].value.clone()
    }
}
fn notice(t: &mut Term, c: ThemeColor, text: &str) -> io::Result<()> {
    Form::new(text, vec![], &["返回"]).interact(t, c, "")?;
    Ok(())
}
fn handshake(client: &mut Client) -> io::Result<ThemeColor> {
    match client.request(Request::Theme)? {
        Reply::Color(rgb) => Ok(ThemeColor(rgb)),
        _ => Err(io::Error::other("Missing session preferences")),
    }
}

pub fn run(file: &Path) -> io::Result<()> {
    terminal::enable_raw_mode()?;
    let _guard = Restore;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let mut term = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    match app(&mut term, file) {
        Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(()),
        r => r,
    }
}
fn app(t: &mut Term, file: &Path) -> io::Result<()> {
    let initial = ThemeColor::default();
    let roots = match bootstrap::configured(file)? {
        Some(r) => r,
        None => {
            let mut setup = super::directory_setup::DirectorySetup::default();
            loop {
                let area = t.draw(|frame| setup.render(frame, initial))?.area;
                match setup.handle(input()?, area, super::native_file::directory) {
                    super::directory_setup::Action::Stay => {}

                    super::directory_setup::Action::Finish(state, work) => {
                        match bootstrap::initialize(file, state, work) {
                            Ok(roots) => break roots,
                            Err(e) => setup.status = e.to_string(),
                        }
                    }
                }
            }
        }
    };
    let mut client = Client::connect(&roots.state)?;
    let mut color = handshake(&mut client)?;
    let new_profile = matches!(client.request(Request::Profile)?, Reply::Team(None));
    if new_profile {
        setup(t, &mut client, color)?
    }
    if matches!(client.request(Request::SetupPending)?, Reply::Pending(true)) {
        registration(t, &mut client, color, true)?
    } else if new_profile {
        identities(t, &mut client, color)?
    }
    let mut board = Bulletin::default();
    let mut published = Vec::new();
    loop {
        let Reply::Drafts(drafts) = client.request(Request::Drafts)? else {
            return Err(io::Error::other("Missing draft list"));
        };
        let draft_views: Vec<_> = drafts
            .iter()
            .map(|d| DraftView {
                reference: &d.reference,
                title: &d.title,
            })
            .collect();
        match client.request(Request::Published) {
            Ok(Reply::Published(letters)) => published = letters,
            Err(e) => {
                notice(t, color, &format!("公告板同步失败：{e}"))?;
            }
            _ => return Err(io::Error::other("Missing publication list")),
        };
        let views: Vec<_> = published
            .iter()
            .map(|l| super::LetterView {
                id: &l.id,
                title: &l.title,
                body: &l.body,
            })
            .collect();
        let rows = BoardRows::new(&draft_views, &views);
        let action = loop {
            let area = t.draw(|f| board.render_with_theme(f, &rows, color))?.area;
            let e = input()?;
            if let Some(action) = board.handle(e, area, &rows) {
                break action;
            }
        };
        match action {
            BoardAction::Compose => compose(t, &mut client, color, Draft::default(), &published)?,
            BoardAction::QuickReply(id) => {
                if let Some(letter) = published.iter().find(|l| l.id == id) {
                    compose(
                        t,
                        &mut client,
                        color,
                        Draft::replying_to(letter, &published),
                        &published,
                    )?
                }
            }
            BoardAction::OpenDraft(reference) => {
                if let Some(d) = drafts.iter().find(|d| d.reference == reference) {
                    compose(t, &mut client, color, d.clone(), &published)?
                }
            }
            BoardAction::Preferences => {
                let mut menu = Form::new("偏好", vec![], &["设置主题色", "身份", "添加成员"]);
                match menu.interact(t, color, "")? {
                    Action::Button(0) => {
                        let mut form = ThemeForm::new(color);
                        loop {
                            let area = t.draw(|f| form.render(f))?.area;
                            match form.handle(input()?, area, |value| {
                                client.request(Request::ApplyTheme(value.0)).map(|_| ())
                            }) {
                                Ok(SettingsAction::Back) => break,
                                Ok(SettingsAction::Stay) => color = form.editor.active(),
                                Err(e) => notice(t, color, &format!("{e:?}"))?,
                            }
                        }
                    }
                    Action::Button(1) => identities(t, &mut client, color)?,
                    Action::Button(_) => registration(t, &mut client, color, false)?,
                    Action::Back => {}
                }
            }
            BoardAction::Workspace => {
                if let Reply::Files(names) = client.request(Request::Workspace)? {
                    workspace(t, &mut client, color, &roots.work, &names)?;
                }
            }
        }
    }
}
fn identities(t: &mut Term, client: &mut Client, color: ThemeColor) -> io::Result<()> {
    let Reply::Team(Some(profile)) = client.request(Request::Profile)? else {
        return Ok(());
    };
    let roster = profile
        .team
        .roster()
        .into_iter()
        .map(|(id, name)| format!("{id} {name}"))
        .collect::<Vec<_>>()
        .join(" · ");
    let mut form = Form::new(
        "选择身份（可留空）",
        vec![
            field("队员编号", profile.human),
            field("助手编号", profile.assistant),
        ],
        &["应用设置"],
    );
    let mut msg = roster;
    loop {
        if matches!(form.interact(t, color, &msg)?, Action::Back) {
            return Ok(());
        }
        match client.request(Request::Identities {
            human: form.val(0),
            assistant: form.val(1),
        }) {
            Ok(_) => return Ok(()),
            Err(e) => msg = e.to_string(),
        }
    }
}
fn registration(
    t: &mut Term,
    client: &mut Client,
    color: ThemeColor,
    initial: bool,
) -> io::Result<()> {
    let buttons: &[&str] = if initial {
        &["添加并激活", "跳过"]
    } else {
        &["添加成员"]
    };
    let mut form = Form::new(
        "添加成员",
        vec![field("人类显示名", ""), field("AI 显示名", "")],
        buttons,
    );
    let mut msg = String::from(if initial {
        "AI 可留空；也可跳过两种身份"
    } else {
        "可单独添加人类或 AI"
    });
    loop {
        let (human, assistant) = match form.interact(t, color, &msg)? {
            Action::Back if !initial => return Ok(()),
            Action::Back => continue,
            Action::Button(1) => (String::new(), String::new()),
            Action::Button(_) => (form.val(0), form.val(1)),
        };
        match client.request(Request::Register {
            human,
            assistant,
            initial,
        }) {
            Ok(_) => return Ok(()),
            Err(e) => msg = e.to_string(),
        }
    }
}
fn setup(t: &mut Term, client: &mut Client, c: ThemeColor) -> io::Result<()> {
    let mut menu = Form::new("队伍设置", vec![], &["新建队伍", "加入已有队伍"]);
    loop {
        let choice = menu.interact(t, c, "")?;
        match choice {
            Action::Back => continue,
            Action::Button(0) => {
                let labels = ["GitHub 账户", "仓库名", "工作区名称（以逗号分隔）"];
                let mut form = Form::new(
                    "新建队伍",
                    labels.iter().map(|s| field(s, "")).collect(),
                    &["私有仓库", "公开仓库"],
                );
                let mut msg = String::new();
                loop {
                    let Action::Button(choice) = form.interact(t, c, &msg)? else {
                        break;
                    };
                    let owner = form.val(0);
                    let name = form.val(1);
                    let spaces = form
                        .val(2)
                        .split([',', '，'])
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect();
                    let manifest =
                        match NewTeam::empty(&format!("team-{}", name.replace('.', "-")), spaces) {
                            Ok(m) => m,
                            Err(e) => {
                                msg = format!("{e:?}");
                                continue;
                            }
                        };
                    let confirm = format!(
                        "确认创建 {owner}/{name}（{}）",
                        if choice == 0 { "私有" } else { "公开" }
                    );
                    if !matches!(
                        Form::new(&confirm, vec![], &["确认创建", "返回"]).interact(t, c, "")?,
                        Action::Button(0)
                    ) {
                        continue;
                    }
                    match client.request(Request::Create {
                        owner,
                        name,
                        public: choice == 1,
                        manifest,
                    }) {
                        Ok(_) => return Ok(()),
                        Err(e) => msg = e.to_string(),
                    }
                }
            }
            Action::Button(_) => {
                let mut form = Form::new(
                    "加入已有队伍",
                    vec![field("GitHub HTTPS 地址", "")],
                    &["加入"],
                );
                let mut msg = String::new();
                loop {
                    if matches!(form.interact(t, c, &msg)?, Action::Back) {
                        break;
                    }
                    match client.request(Request::Join(form.val(0))) {
                        Ok(_) => return Ok(()),
                        Err(e) => msg = e.to_string(),
                    }
                }
            }
        }
    }
}
fn compose(
    t: &mut Term,
    client: &mut Client,
    c: ThemeColor,
    mut saved: Draft,
    board: &[PublishedLetter],
) -> io::Result<()> {
    let Reply::Team(Some(profile)) = client.request(Request::Profile)? else {
        return Err(io::Error::other("Missing team roster"));
    };
    let roster = profile.team.roster();
    let letters: Vec<_> = board
        .iter()
        .map(|l| (l.id.as_str(), l.title.as_str()))
        .collect();
    let mut editing = saved.clone();
    let mut form = Form::new(
        "写信",
        vec![field("标题", &editing.title), field("正文", &editing.body)],
        &[
            "收件人",
            "抄送",
            "引用",
            "前置依赖",
            "添加附件",
            "存草稿",
            "寄信",
        ],
    );
    let mut msg = String::new();
    loop {
        let summary = format!(
            "收件人 {}  抄送 {}  引用 {}  前置依赖 {}  附件 {}\n{msg}",
            editing.to,
            editing.cc,
            editing.references.len(),
            editing.dependencies.len(),
            editing
                .attachments
                .iter()
                .map(|a| a.filename.as_str())
                .collect::<Vec<_>>()
                .join("、")
        );
        let action = form.interact(t, c, &summary)?;
        editing.title = form.val(0);
        editing.body = form.val(1);
        match action {
            Action::Back => {
                if editing == saved
                    || matches!(
                        Form::new("是否丢弃更改", vec![], &["继续编辑", "丢弃更改"])
                            .interact(t, c, "")?,
                        Action::Button(1)
                    )
                {
                    return Ok(());
                }
            }
            Action::Button(0) => {
                if let Some(v) = choose(t, c, "收件人", &roster, &[editing.to.clone()], false)? {
                    editing.to = v.into_iter().next().unwrap_or_default()
                }
            }
            Action::Button(1) => {
                let old = editing
                    .cc
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect::<Vec<_>>();
                if let Some(v) = choose(t, c, "抄送", &roster, &old, true)? {
                    editing.cc = v.join(",")
                }
            }
            Action::Button(2) => {
                if let Some(v) = choose(t, c, "引用", &letters, &editing.references, true)? {
                    editing.references = v
                }
            }
            Action::Button(3) => {
                if let Some(v) = choose(t, c, "前置依赖", &letters, &editing.dependencies, true)?
                {
                    editing.dependencies = v
                }
            }
            Action::Button(4) => match super::native_file::select() {
                Ok(Some(a)) => editing.attachments.push(a),
                Ok(None) => {}
                Err(e) => msg = e.to_string(),
            },
            Action::Button(button) => {
                let send = button == 6;
                if send
                    && !matches!(
                        Form::new("确认寄信", vec![], &["寄信", "返回"]).interact(
                            t,
                            c,
                            &editing.title
                        )?,
                        Action::Button(0)
                    )
                {
                    continue;
                }
                match client.request(Request::SaveDraft(editing.clone())) {
                    Ok(Reply::Saved(d)) => {
                        saved = d.clone();
                        editing = d;
                        msg = "已存于本机".into();
                        if send {
                            match client.request(Request::Send(saved.reference.clone())) {
                                Ok(Reply::Ready) => return Ok(()),
                                Err(e) => msg = e.to_string(),
                                _ => return Err(io::Error::other("Missing send acknowledgement")),
                            }
                        }
                    }
                    Ok(_) => return Err(io::Error::other("Missing draft acknowledgement")),
                    Err(e) => msg = e.to_string(),
                }
            }
        }
    }
}

fn workspace(
    t: &mut Term,
    client: &mut Client,
    color: ThemeColor,
    root: &Path,
    names: &[String],
) -> io::Result<()> {
    let mut focus = 0;
    let mut offset = 0usize;
    let mut status = String::new();
    loop {
        let mut hits = Vec::new();
        t.draw(|f| {
            let [tree, bar, message] = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(f.area());
            let height = usize::from(tree.height.saturating_sub(1));
            offset = offset.min(names.len().saturating_sub(height));
            let lines = names
                .iter()
                .skip(offset)
                .take(height)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            f.render_widget(Paragraph::new(lines).block(super::page("工作区")), tree);
            let buttons = Layout::horizontal([Constraint::Ratio(1, 3); 3]).split(bar);
            for (i, (label, rect)) in ["打开工作目录", "保存工作", "提交工作"]
                .iter()
                .zip(buttons.iter())
                .enumerate()
            {
                let item = Paragraph::new(*label).centered();
                f.render_widget(
                    if focus == i + 1 {
                        item.style(color.highlight())
                    } else {
                        item
                    },
                    *rect,
                );
                hits.push(*rect);
            }
            f.render_widget(Paragraph::new(status.as_str()), message);
        })?;
        let activate = match input()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Up if focus == 0 && offset > 0 => {
                    offset -= 1;
                    false
                }
                KeyCode::Down
                    if focus == 0
                        && offset + usize::from(t.size()?.height.saturating_sub(3))
                            < names.len() =>
                {
                    offset += 1;
                    false
                }
                code if let Some(next) = super::focus_step(code, focus, 4) => {
                    focus = next;
                    false
                }
                KeyCode::Enter => focus > 0,
                _ => false,
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                if let Some(i) = hits
                    .iter()
                    .position(|r| r.contains(Position::new(m.column, m.row)))
                {
                    focus = i + 1;
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if !activate {
            continue;
        }
        if focus == 1 {
            let utility = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            match std::process::Command::new(utility).arg(root).output() {
                Ok(out) if out.status.success() => status.clear(),
                Ok(out) => status = String::from_utf8_lossy(&out.stderr).into_owned(),
                Err(e) => status = e.to_string(),
            }
            continue;
        }
        let Reply::Team(Some(profile)) = client.request(Request::Profile)? else {
            return Err(io::Error::other("Missing profile"));
        };
        let spaces = profile.team.spaces();
        let Some(selection) = choose(t, color, "选择子工作区", &spaces, &[], false)? else {
            continue;
        };
        let Some(space) = selection.first() else {
            continue;
        };
        let mut action = if focus == 2 {
            WorkAction::Save
        } else {
            WorkAction::Submit
        };
        loop {
            let request = Request::WorkspaceAction {
                space: space.clone(),
                action: action.clone(),
            };
            let response = if matches!(action, WorkAction::ResolveOnTerminal(_)) {
                execute!(
                    io::stdout(),
                    DisableMouseCapture,
                    LeaveAlternateScreen,
                    crossterm::cursor::Show
                )?;
                terminal::disable_raw_mode()?;
                let response = client.request(request);
                terminal::enable_raw_mode()?;
                execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
                t.clear()?;
                response
            } else {
                client.request(request)
            };
            match response {
                Ok(Reply::Work(WorkResult::Dirty)) => {
                    if matches!(
                        Form::new("工作区有未保存的修改", vec![], &["取消", "保存并提交"])
                            .interact(t, color, "")?,
                        Action::Button(1)
                    ) {
                        action = WorkAction::SaveAndSubmit
                    } else {
                        break;
                    }
                }
                Ok(Reply::Work(WorkResult::Conflict)) => {
                    action = if matches!(
                        Form::new("文件存在合并冲突", vec![], &["推迟提交", "解决冲突"])
                            .interact(t, color, "")?,
                        Action::Button(1)
                    ) {
                        let tty = std::process::Command::new("tty")
                            .stdin(std::process::Stdio::inherit())
                            .output()?;
                        if !tty.status.success() {
                            status = "无法连接当前终端".into();
                            break;
                        }
                        WorkAction::ResolveOnTerminal(
                            String::from_utf8(tty.stdout)
                                .map_err(io::Error::other)?
                                .trim()
                                .to_owned(),
                        )
                    } else {
                        WorkAction::Defer
                    }
                }
                Ok(Reply::Work(result)) => {
                    status = match result {
                        WorkResult::Saved => "已保存到个人分支",
                        WorkResult::Published => "已提交到 main",
                        WorkResult::Deferred => "已取消本次提交",
                        _ => unreachable!(),
                    }
                    .into();
                    break;
                }
                Err(e) => {
                    status = e.to_string();
                    break;
                }
                _ => return Err(io::Error::other("Missing workspace acknowledgement")),
            }
        }
    }
}

/// Edit a roster or reference selection without mutating the caller's values on Escape.
fn choose(
    t: &mut Term,
    color: ThemeColor,
    title: &str,
    options: &[(&str, &str)],
    existing: &[String],
    multi: bool,
) -> io::Result<Option<Vec<String>>> {
    let mut values = existing.to_vec();
    let mut focus = 0;
    loop {
        let count = options.len() + 1;
        let mut hits = Vec::new();
        t.draw(|f| {
            let block = super::page(title);
            let inner = block.inner(f.area());
            f.render_widget(block, f.area());
            let start = (focus + 1usize).saturating_sub(usize::from(inner.height));
            for (line, index) in (start..count).take(usize::from(inner.height)).enumerate() {
                let rect = Rect::new(inner.x, inner.y + line as u16, inner.width, 1);
                let text = if let Some((id, name)) = options.get(index) {
                    format!(
                        "{} {name}",
                        if values.iter().any(|v| v == id) {
                            "☑"
                        } else {
                            "☐"
                        }
                    )
                } else {
                    "确定".into()
                };
                let label = Paragraph::new(text);
                f.render_widget(
                    if focus == index {
                        label.style(color.highlight())
                    } else {
                        label
                    },
                    rect,
                );
                hits.push((index, rect));
            }
        })?;
        let activate = match input()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Esc => return Ok(None),
                code if let Some(next) = super::focus_step(code, focus, count) => {
                    focus = next;
                    false
                }
                KeyCode::Enter | KeyCode::Char(' ') => true,
                _ => false,
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                if let Some((i, _)) = hits
                    .iter()
                    .find(|(_, r)| r.contains(Position::new(m.column, m.row)))
                {
                    focus = *i;
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if activate {
            let Some((id, _)) = options.get(focus) else {
                return Ok(Some(values));
            };
            if !multi {
                return Ok(Some(vec![id.to_string()]));
            }
            if values.iter().any(|v| v == id) {
                values.retain(|v| v != id)
            } else {
                values.push(id.to_string())
            }
        }
    }
}
