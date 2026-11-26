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
    widgets::{Block, Paragraph, Wrap},
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
                let outer = Block::bordered().title(self.title);
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
                    KeyCode::Tab => self.focus = (self.focus + 1) % count,
                    KeyCode::BackTab => self.focus = (self.focus + count - 1) % count,
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
            let mut form = Form::new(
                "首次设置",
                vec![field("本机状态目录", ""), field("队伍工作目录", "")],
                &["继续"],
            );
            let mut msg = String::new();
            loop {
                if matches!(form.interact(t, initial, &msg)?, Action::Back) {
                    return Ok(());
                }
                match bootstrap::initialize(file, Path::new(&form.val(0)), Path::new(&form.val(1)))
                {
                    Ok(r) => break r,
                    Err(e) => msg = e.to_string(),
                }
            }
        }
    };
    let mut client = Client::connect(&roots.state)?;
    let mut color = handshake(&mut client)?;
    if matches!(client.request(Request::Profile)?, Reply::Team(None)) {
        if !setup(t, &mut client, color)? {
            return Ok(());
        }
        identities(t, &mut client, color)?;
    }
    let mut board = Bulletin::default();
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
        let rows = BoardRows::new(&draft_views, &[]);
        let action = loop {
            let area = t.draw(|f| board.render_with_theme(f, &rows, color))?.area;
            let e = input()?;
            if let Some(action) = board.handle(e, area, &rows) {
                break action;
            }
        };
        match action {
            BoardAction::Compose => compose(t, &mut client, color, Draft::default())?,
            BoardAction::OpenDraft(reference) => {
                if let Some(d) = drafts.iter().find(|d| d.reference == reference) {
                    compose(t, &mut client, color, d.clone())?
                }
            }
            BoardAction::Preferences => {
                let mut menu = Form::new("偏好", vec![], &["设置主题色", "身份"]);
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
                    Action::Button(_) => identities(t, &mut client, color)?,
                    Action::Back => {}
                }
            }
            BoardAction::Workspace => {
                if let Reply::Files(names) = client.request(Request::Workspace)? {
                    workspace(t, color, &roots.work, &names)?;
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
fn setup(t: &mut Term, client: &mut Client, c: ThemeColor) -> io::Result<bool> {
    loop {
        let choice =
            Form::new("队伍设置", vec![], &["新建队伍", "加入已有队伍"]).interact(t, c, "")?;
        match choice {
            Action::Back => return Ok(false),
            Action::Button(0) => {
                let labels = [
                    "GitHub 账户",
                    "仓库名",
                    "队员 1",
                    "队员 2",
                    "队员 3",
                    "助手 1",
                    "助手 2",
                    "助手 3",
                    "工作区名称（以逗号分隔）",
                ];
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
                        .val(8)
                        .split([',', '，'])
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect();
                    let manifest = match NewTeam::from_wizard(
                        &format!("team-{}", name.replace('.', "-")),
                        [form.val(2), form.val(3), form.val(4)],
                        [form.val(5), form.val(6), form.val(7)],
                        spaces,
                    ) {
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
                        Ok(_) => return Ok(true),
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
                        Ok(_) => return Ok(true),
                        Err(e) => msg = e.to_string(),
                    }
                }
            }
        }
    }
}
fn compose(t: &mut Term, client: &mut Client, c: ThemeColor, mut saved: Draft) -> io::Result<()> {
    let mut form = Form::new(
        "写信",
        vec![
            field("标题", &saved.title),
            field("收件人", &saved.to),
            field("抄送", &saved.cc),
            field("正文", &saved.body),
        ],
        &["存草稿"],
    );
    let mut msg = String::new();
    loop {
        let action = form.interact(t, c, &msg)?;
        let current = Draft {
            reference: saved.reference.clone(),
            title: form.val(0),
            to: form.val(1),
            cc: form.val(2),
            body: form.val(3),
        };
        match action {
            Action::Back => {
                if current == saved
                    || matches!(
                        Form::new("是否丢弃更改", vec![], &["继续编辑", "丢弃更改"])
                            .interact(t, c, "")?,
                        Action::Button(1)
                    )
                {
                    return Ok(());
                }
            }
            Action::Button(_) => match client.request(Request::SaveDraft(current)) {
                Ok(Reply::Saved(d)) => {
                    saved = d;
                    msg = "已存于本机".into()
                }
                Ok(_) => return Err(io::Error::other("Missing save acknowledgement")),
                Err(e) => msg = e.to_string(),
            },
        }
    }
}

fn workspace(t: &mut Term, color: ThemeColor, root: &Path, names: &[String]) -> io::Result<()> {
    let mut button = false;
    let mut offset = 0usize;
    let mut error = String::new();
    loop {
        let mut hit = Rect::default();
        t.draw(|f| {
            let [tree, action, status] = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(f.area());
            let height = usize::from(tree.height.saturating_sub(2));
            offset = offset.min(names.len().saturating_sub(height));
            let lines = names
                .iter()
                .skip(offset)
                .take(height)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            f.render_widget(
                Paragraph::new(lines).block(Block::bordered().title("工作区")),
                tree,
            );
            hit = action;
            let label = Paragraph::new("打开工作目录").centered();
            f.render_widget(
                if button {
                    label.style(color.highlight())
                } else {
                    label
                },
                action,
            );
            f.render_widget(Paragraph::new(error.as_str()), status);
        })?;
        let activate = match input()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Tab => {
                    button = !button;
                    false
                }
                KeyCode::Up => {
                    offset = offset.saturating_sub(1);
                    false
                }
                KeyCode::Down => {
                    offset = offset.saturating_add(1);
                    false
                }
                KeyCode::Enter => button,
                _ => false,
            },
            Event::Mouse(m)
                if m.kind == MouseEventKind::Down(MouseButton::Left)
                    && hit.contains(Position::new(m.column, m.row)) =>
            {
                button = true;
                true
            }
            _ => false,
        };
        if activate {
            let utility = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            match std::process::Command::new(utility).arg(root).output() {
                Ok(out) if out.status.success() => error.clear(),
                Ok(out) => error = String::from_utf8_lossy(&out.stderr).into_owned(),
                Err(e) => error = e.to_string(),
            }
        }
    }
}
