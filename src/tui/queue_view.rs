//! A shared footer focus target that suspends the current page.
use super::*;
pub(super) fn step_name(step: DeliveryStep) -> &'static str {
    match step {
        DeliveryStep::Pull => "pull",
        DeliveryStep::Push => "push",
        DeliveryStep::Merge => "merge",
    }
}
pub(super) fn spinner() -> &'static str {
    let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 120;
    frames[(tick % frames.len() as u128) as usize]
}
impl Term {
    pub(super) fn queue(
        &mut self,
    ) -> io::Result<(Vec<DeliveryRow>, Vec<String>, Vec<RemoteActivity>)> {
        let Some(client) = &mut self.status else {
            return Ok((Vec::new(), Vec::new(), Vec::new()));
        };
        match client.request(Request::DeliveryQueue)? {
            Reply::DeliveryQueue(rows, completed, activities) => Ok((rows, completed, activities)),
            _ => Err(io::Error::other("Missing task queue")),
        }
    }
    pub(in crate::tui) fn page_input(
        &mut self,
        last: bool,
        color: ThemeColor,
    ) -> io::Result<Event> {
        self.theme = color;
        let event = input()?;
        if let Event::Mouse(m) = &event
            && m.kind == MouseEventKind::Down(MouseButton::Left)
            && self.footer.contains(Position::new(m.column, m.row))
        {
            self.footer_focus = true;
            return Ok(Event::FocusGained);
        }
        if self.footer_focus {
            if let Event::Key(k) = &event
                && k.kind == KeyEventKind::Press
            {
                match k.code {
                    KeyCode::Enter => {
                        self.show_queue()?;
                        self.footer_focus = false;
                    }
                    KeyCode::Esc | KeyCode::BackTab | KeyCode::Up | KeyCode::Left => {
                        self.footer_focus = false
                    }
                    KeyCode::Tab => {
                        self.footer_focus = false;
                        return Ok(event);
                    }
                    _ => {}
                }
            } else if matches!(event, Event::Mouse(_)) {
                self.footer_focus = false;
                return Ok(event);
            }
            return Ok(Event::FocusGained);
        }
        if last
            && matches!(event,Event::Key(k) if k.kind==KeyEventKind::Press && matches!(k.code,KeyCode::Tab|KeyCode::Down))
        {
            self.footer_focus = true;
            return Ok(Event::FocusGained);
        }
        Ok(event)
    }
    pub(super) fn show_queue(&mut self) -> io::Result<()> {
        let mut offset = 0usize;
        loop {
            let (jobs, completed, activities) = self.queue()?;
            let mut lines = Vec::new();
            for row in jobs {
                let mut active = false;
                for step in [DeliveryStep::Pull, DeliveryStep::Push, DeliveryStep::Merge] {
                    let (mark, note) = match &row.state {
                        DeliveryState::Waiting => ("等待", ""),
                        DeliveryState::Running(s) if *s == step => {
                            active = true;
                            (spinner(), "")
                        }
                        DeliveryState::Failed(s, e) if *s == step => {
                            active = true;
                            ("!", e.as_str())
                        }
                        _ if active => (
                            if matches!(row.state, DeliveryState::Failed(..)) {
                                "未执行"
                            } else {
                                "等待"
                            },
                            "",
                        ),
                        _ => continue,
                    };
                    lines.push(format!(
                        "{mark}  {} · {}  {note}",
                        step_name(step),
                        row.title
                    ));
                }
            }
            lines.extend(activities.into_iter().map(|a| {
                format!(
                    "{}  {}",
                    match a.state {
                        ActivityState::Running => spinner(),
                        ActivityState::Waiting => "等待",
                        ActivityState::Failed(_) => "!",
                    },
                    match a.state {
                        ActivityState::Failed(reason) => format!("{} · {reason}", a.label),
                        _ => a.label,
                    }
                )
            }));
            lines.extend(completed.into_iter().map(|s| format!("完成  {s}")));
            if lines.is_empty() {
                lines.push("队列为空".into());
            }
            let mut page = 1;
            self.terminal.draw(|f| {
                let header = super::super::page("任务队列", self.theme);
                let area = header.inner(f.area());
                f.render_widget(header, f.area());
                let [body, hint] =
                    Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
                page = usize::from(body.height.max(1));
                offset = offset.min(lines.len().saturating_sub(page));
                f.render_widget(
                    Paragraph::new(
                        lines
                            .iter()
                            .skip(offset)
                            .take(page)
                            .map(|s| ratatui::text::Line::raw(s.as_str()))
                            .collect::<Vec<_>>(),
                    ),
                    body,
                );
                f.render_widget(Paragraph::new("↑↓ · 滚动    esc · 返回"), hint);
            })?;
            match input()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    KeyCode::Esc => return Ok(()),
                    KeyCode::Down => offset += 1,
                    KeyCode::Up => offset = offset.saturating_sub(1),
                    KeyCode::PageDown => offset += page,
                    KeyCode::PageUp => offset = offset.saturating_sub(page),
                    _ => {}
                },
                Event::Mouse(m) if m.kind == MouseEventKind::ScrollDown => offset += 3,
                Event::Mouse(m) if m.kind == MouseEventKind::ScrollUp => {
                    offset = offset.saturating_sub(3)
                }
                _ => {}
            }
        }
    }
}
