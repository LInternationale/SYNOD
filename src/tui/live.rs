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
    help: &'a str,
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
            help: "",
        }
    }
    fn interact(&mut self, t: &mut Term, color: ThemeColor, message: &str) -> io::Result<Action> {
        let count = self.fields.len() + self.buttons.len();
        let mut bounds = Vec::new();
        loop {
            t.draw(|f| {
                let area = f.area();
                let outer = super::page(self.title, color);
                let inner = outer.inner(area);
                f.render_widget(outer, area);
                let help = Paragraph::new(self.help).wrap(Wrap { trim: false });
                let h = if self.help.is_empty() {
                    0
                } else {
                    self.help
                        .lines()
                        .map(|line| {
                            ratatui::text::Span::raw(line)
                                .width()
                                .div_ceil(usize::from(inner.width.max(1)))
                                .max(1)
                        })
                        .sum::<usize>()
                        .min(usize::from(inner.height.saturating_sub(4))) as u16
                };
                let [help_area, inner] =
                    Layout::vertical([Constraint::Length(h), Constraint::Min(0)]).areas(inner);
                f.render_widget(help, help_area);
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
                    super::input_field::render(
                        f,
                        rect,
                        field.label,
                        &field.value,
                        i == self.focus,
                        color,
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
                    code if let Some(next) =
                        super::focus_step(code, self.focus, count, &bounds) =>
                    {
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
    loop {
        t.draw(|f| {
            f.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(super::page("提示", c)),
                f.area(),
            )
        })?;
        if matches!(input()?,Event::Key(k) if k.kind==KeyEventKind::Press && k.code==KeyCode::Esc) {
            break;
        }
    }
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
    let mut filter = "all".to_string();
    let mut last_error = None;
    'refresh: loop {
        if let Reply::SyncStatus(error) = client.request(Request::SyncStatus)? {
            if error != last_error
                && let Some(e) = &error
            {
                notice(t, color, &format!("后台同步未完成：{e}"))?;
            }
            last_error = error;
        }
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
        let actor = if filter == "all" {
            String::new()
        } else {
            match client.request(Request::Profile)? {
                Reply::Team(Some(p)) => p.human,
                _ => String::new(),
            }
        };
        let labels: std::collections::HashMap<_, _> = published
            .iter()
            .filter(|l| l.unread)
            .map(|l| (&l.id, format!("【未读】{}", l.title)))
            .collect();
        let views: Vec<_> = published
            .iter()
            .filter(|l| match filter.as_str() {
                "to_me" => !actor.is_empty() && l.to == actor,
                "cc_me" => l.cc.contains(&actor),
                _ => true,
            })
            .map(|l| super::LetterView {
                id: &l.id,
                title: labels.get(&l.id).map_or(l.title.as_str(), String::as_str),
                body: &l.body,
            })
            .collect();
        let rows = BoardRows::new(if filter == "all" { &draft_views } else { &[] }, &views);
        let action = loop {
            let area = t.draw(|f| board.render_with_theme(f, &rows, color))?.area;
            if !event::poll(std::time::Duration::from_secs(2))? {
                continue 'refresh;
            }
            let e = input()?;
            if let Some(action) = board.handle(e, area, &rows) {
                break action;
            }
        };
        match action {
            BoardAction::Read(id) => {
                if let Reply::Letter(l) = client.request(Request::Read(id))? {
                    task_detail(t, &mut client, color, &l, &published)?
                }
                board.return_to_list();
            }
            BoardAction::Filter => {
                if let Some(choice) = choose(
                    t,
                    color,
                    "筛选信件",
                    &[
                        ("all", "全部信件"),
                        ("to_me", "发给我的"),
                        ("cc_me", "抄送我的"),
                    ],
                    &[filter.clone()],
                    false,
                )? && let Some(value) = choice.first()
                {
                    filter = value.clone()
                }
            }
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
                let mut menu = Form::new(
                    "偏好",
                    vec![],
                    &["设置主题色", "身份", "添加成员", "同步间隔", "子工作区标签"],
                );
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
                    Action::Button(2) => registration(t, &mut client, color, false)?,
                    Action::Button(3) => {
                        if let Reply::Interval(seconds) = client.request(Request::Interval)? {
                            let mut form = Form::new(
                                "后台同步间隔",
                                vec![field("检查间隔（秒）", seconds.to_string())],
                                &["应用设置"],
                            );
                            let mut error = String::new();
                            while let Action::Button(_) = form.interact(t, color, &error)? {
                                match form.val(0).parse() {
                                    Ok(value) => {
                                        client.request(Request::SetInterval(value))?;
                                        break;
                                    }
                                    Err(_) => error = "请填写大于零的整数秒数".into(),
                                }
                            }
                        }
                    }
                    Action::Button(_) => workspace(t, &mut client, color, &roots.work, true)?,
                    Action::Back => {}
                }
            }
            BoardAction::Workspace => {
                workspace(t, &mut client, color, &roots.work, false)?;
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
    menu.help = "如果你是第一个开始协作的人，选择「新建队伍」. 队友已建好队伍并发给你 GitHub 链接时，选择「加入已有队伍」. ";
    loop {
        let choice = menu.interact(t, c, "")?;
        match choice {
            Action::Back => continue,
            Action::Button(0) => {
                if create_team_wizard(t, client, c)? {
                    return Ok(());
                }
            }
            Action::Button(_) => {
                let mut form =
                    Form::new("加入已有队伍", vec![field("队伍仓库链接", "")], &["加入"]);
                form.help = "粘贴队友发给你的仓库链接，格式为 https://github.com/账号/仓库名. ";
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
/// Return true only after the final confirmation has provisioned the repository.
fn create_team_wizard(t: &mut Term, client: &mut Client, color: ThemeColor) -> io::Result<bool> {
    use crate::adapters::github::{RepositoryRequest, Visibility};
    let mut owner = Form::new(
        "新建队伍 · 1/4 · 选择存放位置",
        vec![field("GitHub 用户名或组织名", "")],
        &["下一步"],
    );
    owner.help = "队伍的信件和文件会保存在 GitHub 上. 填写你的 GitHub 用户名，或你可以创建仓库的组织名. 比如个人主页是 github.com/alice，就填 alice. ";
    let mut name = Form::new(
        "新建队伍 · 2/4 · 给队伍仓库起名",
        vec![field("仓库名", "")],
        &["下一步"],
    );
    name.help = "仓库是队伍在 GitHub 上存放数据的地方. 名称会成为网址的一部分，例如 contest-team. 可用英文字母、数字、短横线、下划线和点. ";
    let mut access = Form::new(
        "新建队伍 · 3/4 · 谁能查看队伍数据",
        vec![],
        &["仅有权限的人", "任何人"],
    );
    access.help = "选择「仅有权限的人」，只有获准访问这个 GitHub 仓库的人能查看. 选择「任何人」，互联网上的任何人都能查看队伍的信件、草稿、附件和共享文件. ";
    let mut step = 0usize;
    let mut public = false;
    let mut error = String::new();
    loop {
        let overview = format!(
            "队伍数据将存放在 github.com/{}/{}\n可查看数据的人：{}\n创建后，再添加本机要使用的人类和 AI 身份. ",
            owner.val(0),
            name.val(0),
            if public {
                "任何人"
            } else {
                "仅有仓库访问权限的人"
            },
        );
        let action = match step {
            0 => owner.interact(t, color, &error)?,
            1 => name.interact(t, color, &error)?,
            2 => access.interact(t, color, &error)?,
            _ => {
                let mut review =
                    Form::new("新建队伍 · 4/4 · 核对并创建", vec![], &["确认创建队伍"]);
                review.help = &overview;
                review.interact(t, color, &error)?
            }
        };
        error.clear();
        let Action::Button(choice) = action else {
            if step == 0 {
                return Ok(false);
            }
            step -= 1;
            continue;
        };
        match step {
            0 if RepositoryRequest::new(&owner.val(0), "team", Visibility::Private).is_err() => {
                error = "请填写 GitHub 用户名或组织名，例如 alice. ".into();
                continue;
            }
            1 if RepositoryRequest::new(&owner.val(0), &name.val(0), Visibility::Private)
                .is_err() =>
            {
                error = "请填写可以用在仓库网址里的名称，例如 contest-team. ".into();
                continue;
            }
            2 => public = choice == 1,
            3 => {
                let manifest = NewTeam::empty(
                    &format!("team-{}", name.val(0).replace('.', "-")),
                    Vec::new(),
                )
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
                match client.request(Request::Create {
                    owner: owner.val(0),
                    name: name.val(0),
                    public,
                    manifest,
                }) {
                    Ok(_) => return Ok(true),
                    Err(e) => {
                        error = e.to_string();
                        continue;
                    }
                }
            }
            _ => {}
        }
        step += 1;
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
            "子任务",
            "添加附件",
            "存草稿",
            "寄信",
        ],
    );
    let mut msg = String::new();
    loop {
        let summary = format!(
            "收件人 {}  抄送 {}  引用 {}  子任务 {}  附件 {}\n{msg}",
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
                if let Some(v) = choose(t, c, "子任务", &letters, &editing.dependencies, true)? {
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
                        Form::new("确认寄信", vec![], &["寄信"]).interact(t, c, &editing.title)?,
                        Action::Button(0)
                    )
                {
                    continue;
                }
                match client.request(Request::SaveDraft(editing.clone())) {
                    Ok(Reply::Saved(d, pending)) => {
                        saved = d.clone();
                        editing = d;
                        if let Some(reason) = pending {
                            msg = format!("已存于本机，等待同步. {reason}");
                            continue;
                        }
                        msg = "草稿已存至云端".into();
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
    tags_only: bool,
) -> io::Result<()> {
    use crate::adapters::git::workspace_sync::Mode;
    let mut focus = 0usize;
    let mut selected = String::new();
    let mut status = String::new();
    let mut refresh = true;
    let mut names = Vec::new();
    let mut tags = std::collections::BTreeMap::new();
    let mut modes = std::collections::BTreeMap::new();
    let labels = if tags_only {
        vec!["切换标签"]
    } else {
        vec!["打开工作目录", "保存工作", "提交工作", "获取更新"]
    };
    loop {
        if refresh {
            if let Reply::Files(files) = client.request(Request::Workspace)? {
                names = files;
            }
            if let Reply::Tags(list) = client.request(Request::Tags)? {
                tags = list;
            }
            modes.clear();
            for tag in tags.keys() {
                if !names.contains(tag) {
                    names.push(tag.clone());
                }
                if let Ok(Reply::SpaceSync(p)) = client.request(Request::SpaceSync {
                    space: tag.clone(),
                    action: SyncAction::Preview,
                }) {
                    modes.insert(tag.clone(), p);
                }
            }
            names.sort();
            if !names.contains(&selected) {
                selected = names.first().cloned().unwrap_or_default();
            }
            refresh = false;
        }
        let index = names.iter().position(|s| *s == selected).unwrap_or(0);
        let mut bounds = Vec::new();
        let mut rows = Vec::new();
        t.draw(|f| {
            let block = super::page(
                if tags_only {
                    "子工作区标签"
                } else {
                    "工作区"
                },
                color,
            );
            let inner = block.inner(f.area());
            f.render_widget(block, f.area());
            let [tree, bar, footer] = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(2),
            ])
            .areas(inner);
            bounds.push((0, tree));
            let count = usize::from(tree.height);
            for (n, name) in names
                .iter()
                .enumerate()
                .skip((index + 1).saturating_sub(count))
                .take(count)
            {
                let mode = modes.get(name);
                let label = if tags.contains_key(name) {
                    let m = if mode.is_some_and(|p| p.mode == Mode::WhenClean) {
                        "无修改时自动同步"
                    } else {
                        "仅手动"
                    };
                    let update = match mode {
                        Some(p) if !p.paths.is_empty() && p.dirty => " · 有更新，本地有修改",
                        Some(p) if !p.paths.is_empty() => " · 有更新",
                        _ => "",
                    };
                    format!("{name}/  [{m}{update}]")
                } else if root.join(name).is_dir() {
                    format!("{name}/")
                } else {
                    name.clone()
                };
                let rect = Rect::new(tree.x, tree.y + rows.len() as u16, tree.width, 1);
                let row = Paragraph::new(label);
                f.render_widget(
                    if focus == 0 && n == index {
                        row.style(color.highlight())
                    } else {
                        row
                    },
                    rect,
                );
                rows.push((n, rect));
            }
            for (i, (label, rect)) in labels
                .iter()
                .zip(
                    Layout::horizontal(vec![
                        Constraint::Ratio(1, labels.len() as u32);
                        labels.len()
                    ])
                    .split(bar)
                    .iter(),
                )
                .enumerate()
            {
                let w = Paragraph::new(*label).centered();
                f.render_widget(
                    if focus == i + 1 {
                        w.style(color.highlight())
                    } else {
                        w
                    },
                    *rect,
                );
                bounds.push((i + 1, *rect));
            }
            let help = if tags_only {
                "选中文件夹，按空格添加或移除子工作区标签"
            } else {
                "选中子工作区，按空格切换同步模式"
            };
            f.render_widget(
                Paragraph::new(if status.is_empty() { help } else { &status }),
                footer,
            );
        })?;
        if !event::poll(std::time::Duration::from_secs(2))? {
            refresh = true;
            continue;
        }
        let toggle = match input()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Up if focus == 0 && index > 0 => {
                    selected = names[index - 1].clone();
                    continue;
                }
                KeyCode::Down if focus == 0 && index + 1 < names.len() => {
                    selected = names[index + 1].clone();
                    continue;
                }
                KeyCode::Char(' ') if focus == 0 => true,
                KeyCode::Enter if focus > 0 => tags_only,
                code if let Some(next) =
                    super::focus_step(code, focus, labels.len() + 1, &bounds) =>
                {
                    focus = next;
                    continue;
                }
                _ => continue,
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                let pos = Position::new(m.column, m.row);
                if let Some((n, _)) = rows.iter().find(|(_, r)| r.contains(pos)) {
                    selected = names[*n].clone();
                    focus = 0;
                    continue;
                }
                let Some((button, _)) = bounds.iter().skip(1).find(|(_, r)| r.contains(pos)) else {
                    continue;
                };
                focus = *button;
                tags_only
            }
            _ => continue,
        };
        refresh = true;
        if toggle {
            let request = if tags_only {
                Request::ToggleTag(selected.clone())
            } else if let Some(p) = modes.get(&selected) {
                Request::SpaceSync {
                    space: selected.clone(),
                    action: SyncAction::SetMode(if p.mode == Mode::Manual {
                        Mode::WhenClean
                    } else {
                        Mode::Manual
                    }),
                }
            } else {
                status = "请选择已标记的子工作区".into();
                continue;
            };
            status = client
                .request(request)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
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
        let space = &selected;
        if !tags.contains_key(space) {
            status = "请先选中一个子工作区目录".into();
            continue;
        }
        if focus == 4 {
            let response = client.request(Request::SpaceSync {
                space: space.clone(),
                action: SyncAction::RefreshPreview,
            });
            let plan = match response {
                Ok(Reply::SpaceSync(p)) => p,
                Err(e) => {
                    status = e.to_string();
                    continue;
                }
                _ => return Err(io::Error::other("Missing workspace preview")),
            };
            if plan.paths.is_empty() {
                status = "已是最新版本".into();
                continue;
            }
            if !plan.conflicts.is_empty() {
                notice(
                    t,
                    color,
                    &format!("以下文件两边都有修改：\n{}", plan.conflicts.join("\n")),
                )?;
                continue;
            }
            let mut form = Form::new("获取更新", vec![], &["取消", "应用更新"]);
            let paths = plan.paths.join("\n");
            form.help = &paths;
            if let Action::Button(1) = form.interact(t, color, "")? {
                status = match client.request(Request::SpaceSync {
                    space: space.clone(),
                    action: SyncAction::Apply(plan),
                }) {
                    Ok(_) => "已应用更新".into(),
                    Err(e) => e.to_string(),
                };
            }
            continue;
        }
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
                        WorkResult::Saved => "已保存工作",
                        WorkResult::Published => "已提交到共享工作区",
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
            let block = super::page(title, color);
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
                KeyCode::Up => {
                    focus = focus.saturating_sub(1);
                    false
                }
                KeyCode::Down => {
                    focus = (focus + 1).min(count - 1);
                    false
                }
                code if let Some(next) = super::focus_step(code, focus, count, &hits) => {
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

/// Inspect the signed original and stage the manual completion flag separately from progress.
fn task_detail(
    t: &mut Term,
    client: &mut Client,
    color: ThemeColor,
    initial: &PublishedLetter,
    board: &[PublishedLetter],
) -> io::Result<()> {
    use crate::application::letters::TaskChange;
    let mut letter = initial.clone();
    let mut staged = letter.completed;
    let mut ticks: Vec<_> = letter.checklist.iter().map(|i| i.checked).collect();
    let mut reveal = false;
    let mut focus = 0usize;
    let mut message = String::new();
    let mut scroll = 0u16;
    loop {
        let check = if staged {
            "☑ 任务完成"
        } else {
            "☐ 任务完成"
        };
        let mut bounds = Vec::new();
        t.draw(|f| {
            let outer = super::page(&letter.title, color);
            let inner = outer.inner(f.area());
            f.render_widget(outer, f.area());
            let show_progress = !letter.dependencies.is_empty() || !letter.checklist.is_empty();
            let [meter, body, bar, footer] = Layout::vertical([
                Constraint::Length(u16::from(show_progress)),
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(2),
            ])
            .areas(inner);
            if show_progress {
                f.render_widget(
                    ratatui::widgets::Gauge::default()
                        .gauge_style(color.highlight())
                        .ratio(letter.progress_done as f64 / letter.progress_total as f64)
                        .label(format!(
                            "任务进度  {}/{}  ({}%)",
                            letter.progress_done, letter.progress_total, letter.progress
                        )),
                    meter,
                );
            }
            let mut text = format!(
                "发件人  {}\n收件人  {}\n抄送  {}\n",
                letter.author,
                letter.to,
                letter.cc.join("、")
            );
            let mut lines: Vec<ratatui::text::Line> =
                text.split('\n').map(|s| s.to_owned().into()).collect();
            let mut positions = Vec::new();
            let mut next_item = letter.checklist.iter().enumerate().peekable();
            for (line, raw) in letter.body.lines().enumerate() {
                if let Some(&(i, item)) = next_item.peek()
                    && item.line == line
                {
                    positions.push((i + 2, lines.len()));
                    let style = if focus == i + 2 {
                        color.highlight()
                    } else {
                        Default::default()
                    };
                    lines.push(ratatui::text::Line::styled(
                        format!("{} {}", if ticks[i] { "☑" } else { "☐" }, item.label),
                        style,
                    ));
                    next_item.next();
                } else {
                    lines.push(raw.to_owned().into());
                }
            }
            text.clear();
            if !letter.references.is_empty() {
                text.push_str("\n\n引用");
                for id in &letter.references {
                    text.push_str("\n");
                    text.push_str(
                        board
                            .iter()
                            .find(|l| &l.id == id)
                            .map_or(id.as_str(), |l| l.title.as_str()),
                    );
                }
            }
            for event in letter
                .events
                .iter()
                .filter(|e| matches!(e.kind.as_str(), "MarkedCompleted" | "MarkedIncomplete"))
            {
                text.push_str(&format!(
                    "\n{}  {}  {}",
                    event.occurred_at,
                    event.actor_id,
                    if event.kind == "MarkedCompleted" {
                        "标记完成"
                    } else {
                        "取消完成"
                    }
                ));
            }
            lines.extend(text.lines().map(|s| s.to_owned().into()));
            let heights: Vec<_> = lines
                .iter()
                .scan(0usize, |offset, line| {
                    let y = *offset;
                    *offset += Paragraph::new(line.clone())
                        .wrap(Wrap { trim: false })
                        .line_count(body.width);
                    Some((y, *offset))
                })
                .collect();
            if reveal && let Some((_, line)) = positions.iter().find(|(i, _)| *i == focus) {
                let y = heights[*line].0.min(u16::MAX as usize) as u16;
                scroll = scroll
                    .min(y)
                    .max(y.saturating_sub(body.height.saturating_sub(1)));
            }
            reveal = false;
            scroll = scroll.min(heights.last().map_or(0, |h| {
                h.1.saturating_sub(body.height as usize)
                    .min(u16::MAX as usize) as u16
            }));
            for (id, line) in positions {
                let (start, end) = heights[line];
                let from = start.max(scroll as usize);
                let to = end.min(scroll as usize + body.height as usize);
                if from < to {
                    bounds.push((
                        id,
                        Rect::new(
                            body.x,
                            body.y + (from - scroll as usize) as u16,
                            body.width,
                            (to - from) as u16,
                        ),
                    ));
                }
            }
            f.render_widget(
                Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll, 0)),
                body,
            );
            let zones = Layout::horizontal([Constraint::Ratio(1, 2); 2]).split(bar);
            for (i, (label, rect)) in [check, "进度设置"].iter().zip(zones.iter()).enumerate() {
                let w = Paragraph::new(*label).centered();
                f.render_widget(
                    if i == focus {
                        w.style(color.highlight())
                    } else {
                        w
                    },
                    *rect,
                );
                bounds.push((i, *rect));
            }
            f.render_widget(
                Paragraph::new(if message.is_empty() {
                    "Space 切换勾选，Enter 提交"
                } else {
                    &message
                }),
                footer,
            );
        })?;
        let submit = match input()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Char(' ') if focus >= 2 => {
                    ticks[focus - 2] = !ticks[focus - 2];
                    false
                }
                KeyCode::Char(' ') if focus == 0 => {
                    staged = !staged;
                    message.clear();
                    false
                }
                KeyCode::PageUp => {
                    scroll = scroll.saturating_sub(5);
                    false
                }
                KeyCode::PageDown => {
                    scroll = scroll.saturating_add(5);
                    false
                }
                KeyCode::Enter => true,
                KeyCode::Up if focus > 2 => {
                    focus -= 1;
                    reveal = true;
                    false
                }
                KeyCode::Down if focus >= 2 && focus < ticks.len() + 1 => {
                    focus += 1;
                    reveal = true;
                    false
                }
                KeyCode::Up if focus < 2 && !ticks.is_empty() => {
                    focus = ticks.len() + 1;
                    reveal = true;
                    false
                }
                code if let Some(next) =
                    super::focus_step(code, focus, 2 + ticks.len(), &bounds) =>
                {
                    focus = next;
                    reveal = true;
                    false
                }
                _ => false,
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                if let Some((i, _)) = bounds
                    .iter()
                    .find(|(_, r)| r.contains(Position::new(m.column, m.row)))
                {
                    focus = *i;
                    if focus >= 2 {
                        ticks[focus - 2] = !ticks[focus - 2];
                        false
                    } else if focus == 0 {
                        staged = !staged;
                        false
                    } else {
                        true
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        if !submit {
            continue;
        }
        let change = if focus == 0 {
            if staged == letter.completed {
                continue;
            }
            TaskChange::Completion(staged)
        } else if focus >= 2 {
            let item = &letter.checklist[focus - 2];
            if item.checked == ticks[focus - 2] {
                continue;
            }
            TaskChange::Checklist(crate::application::letters::ChecklistChange {
                line: item.line,
                checked: ticks[focus - 2],
            })
        } else {
            let options = board
                .iter()
                .filter(|l| l.id != letter.id)
                .map(|l| (l.id.as_str(), l.title.as_str()))
                .collect::<Vec<_>>();
            let Some(selected) = choose(
                t,
                color,
                "进度设置 · 选择子任务",
                &options,
                &letter.dependencies,
                true,
            )?
            else {
                continue;
            };
            TaskChange::Subtasks {
                selected,
                observed: letter.dependencies.clone(),
            }
        };
        match client.request(Request::Task {
            letter: letter.id.clone(),
            change,
        }) {
            Ok(Reply::Ready) => {
                message = "已同步".into();
                if let Reply::Published(letters) = client.request(Request::Published)?
                    && let Some(updated) = letters.into_iter().find(|l| l.id == letter.id)
                {
                    for (i, item) in updated.checklist.iter().enumerate() {
                        if focus == i + 2 || ticks[i] == letter.checklist[i].checked {
                            ticks[i] = item.checked;
                        }
                    }
                    if focus == 0 || staged == letter.completed {
                        staged = updated.completed;
                    }
                    letter = updated;
                }
            }
            Err(e) => message = e.to_string(),
            _ => return Err(io::Error::other("Missing task acknowledgement")),
        }
    }
}
