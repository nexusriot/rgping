use std::collections::VecDeque;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Line},
    widgets::{Block, Borders, Paragraph, Chart, Axis, Dataset, GraphType},
};
use crate::pinger::PingSample;

pub struct UiConfig {
    pub host: String,
    pub history: usize,
}

pub struct UiState {
    rtts: VecDeque<Option<f64>>,
    total: u64,
    lost: u64,
    last: Option<f64>,
}

impl UiState {
    pub fn new(history: usize) -> Self {
        Self {
            rtts: VecDeque::with_capacity(history),
            total: 0,
            lost: 0,
            last: None,
        }
    }

    pub fn push(&mut self, rtt: Option<f64>, history: usize) {
        self.total += 1;
        if rtt.is_none() { self.lost += 1; }
        self.last = rtt;
        if self.rtts.len() == history {
            self.rtts.pop_front();
        }
        self.rtts.push_back(rtt);
    }

    pub fn avg(&self) -> Option<f64> {
        let mut sum = 0.0;
        let mut cnt = 0;
        for v in self.rtts.iter().flatten() {
            sum += *v;
            cnt += 1;
        }
        (cnt > 0).then(|| sum / cnt as f64)
    }

    pub fn loss_pct(&self) -> f64 {
        if self.total == 0 { 0.0 } else { (self.lost as f64) * 100.0 / (self.total as f64) }
    }

    fn y_max(&self) -> f64 {
        let mut m = 10.0;
        for v in self.rtts.iter().flatten() {
            if *v > m { m = *v; }
        }
        (m * 1.20).ceil()
    }
}

pub struct Ui {
    cfg: UiConfig,
    state: UiState,
}

impl Ui {
    pub fn new(cfg: UiConfig) -> Self {
        Self { state: UiState::new(cfg.history), cfg }
    }

    pub fn push(&mut self, s: &PingSample) {
        self.state.push(s.rtt_ms, self.cfg.history);
    }

    pub fn run_tui(mut self, mut rx: tokio::sync::mpsc::Receiver<PingSample>) -> anyhow::Result<()> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let res = 'outer: loop {
            while event::poll(std::time::Duration::from_millis(10))? {
                if let Event::Key(k) = event::read()? {
                    if k.code == KeyCode::Char('q')
                        || k.code == KeyCode::Esc
                        || (k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        break 'outer Ok(());
                    }
                }
            }

            while let Ok(s) = rx.try_recv() {
                self.push(&s);
            }

            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Percentage(100),
                        Constraint::Length(2),
                    ].as_ref())
                    .split(f.size());

                let header = Paragraph::new(vec![
                    Line::from(vec![
                        Span::styled("rgping  ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::raw(format!("host: {}", self.cfg.host)),
                    ]),
                ]).block(Block::default().borders(Borders::ALL).title(" Info "));
                f.render_widget(header, chunks[0]);

                let points: Vec<(f64, f64)> = self.state.rtts.iter()
                    .enumerate()
                    .filter_map(|(i, v)| v.map(|ms| (i as f64, ms)))
                    .collect();

                let y_max = self.state.y_max();
                let x_max = self.cfg.history as f64;

                let dataset = Dataset::default()
                    .name("RTT (ms)")
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Green))
                    .data(&points);

                let chart = Chart::new(vec![dataset])
                    .block(Block::default().borders(Borders::ALL).title(" Latency "))
                    .x_axis(
                        Axis::default()
                            .title("Samples")
                            .bounds([0.0, x_max])
                            .labels(vec![
                                Span::raw("0"),
                                Span::raw(format!("{}", self.cfg.history / 2)),
                                Span::raw(format!("{}", self.cfg.history)),
                            ])
                    )
                    .y_axis(
                        Axis::default()
                            .title("RTT (ms)")
                            .bounds([0.0, y_max])
                            .labels(vec![
                                Span::raw("0"),
                                Span::raw(format!("{:.0}", y_max / 2.0)),
                                Span::raw(format!("{:.0}", y_max)),
                            ])
                    );

                f.render_widget(chart, chunks[1]);

                let last = self.state.last.map(|v| format!("{v:.1} ms")).unwrap_or_else(|| "timeout".into());
                let avg  = self.state.avg().map(|v| format!("{v:.1} ms")).unwrap_or_else(|| "-".into());
                let loss = format!("{:.1}%", self.state.loss_pct());

                let foot = Paragraph::new(Line::from(vec![
                    Span::raw("last: "), Span::styled(last, Style::default().fg(Color::Green)),
                    Span::raw("   avg: "),  Span::styled(avg,  Style::default().fg(Color::Yellow)),
                    Span::raw("   loss: "), Span::styled(loss, Style::default().fg(Color::Red)),
                    Span::raw("   quit: q / Esc / Ctrl-C"),
                ])).block(Block::default().borders(Borders::ALL));
                f.render_widget(foot, chunks[2]);
            })?;
        };

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
        terminal.show_cursor()?;
        res
    }
}
