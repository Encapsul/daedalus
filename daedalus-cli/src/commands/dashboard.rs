use anyhow::Result;
use clap::Args;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style, Stylize},
    symbols::bar as bar_sym,
    text::{Line, Text},
    widgets::{block::Title, Block, BorderType, Cell, Paragraph, Row, Table},
    Frame,
};
use std::path::PathBuf;

#[derive(Args)]
pub struct DashboardArgs {
    /// Path to results.csv (default: examples/benchmark/results.csv)
    #[arg(short, long)]
    pub csv: Option<PathBuf>,

    /// daedalus cache directory (default: ~/.cache/daedalus)
    #[arg(short = 'C', long)]
    pub cache: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct BenchmarkRow {
    model: String,
    full_mb: f64,
    delta_mb: f64,
    saved_pct: f64,
    total_chunks: u64,
    reused_chunks: u64,
    fetched_chunks: u64,
}

pub fn run(args: &DashboardArgs) -> Result<()> {
    let rows = load_benchmarks(&args.csv)?;
    if rows.is_empty() {
        anyhow::bail!(
            "no benchmark data found — run `python3 examples/benchmark/plot_results.py` first \
             or pass --csv path"
        );
    }

    let cache_info = load_cache_info(&args.cache);

    let mut terminal = ratatui::init();
    let _guard = ShutdownGuard;

    let mut should_quit = false;
    while !should_quit {
        terminal.draw(|frame| ui(frame, &rows, &cache_info))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

struct ShutdownGuard;

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

fn load_benchmarks(csv_path: &Option<PathBuf>) -> Result<Vec<BenchmarkRow>> {
    let path = csv_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("examples/benchmark/results.csv"));

    let content = std::fs::read_to_string(&path)?;
    let mut rows = Vec::new();

    for line in content.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 8 {
            continue;
        }
        rows.push(BenchmarkRow {
            model: cols[0].to_string(),
            full_mb: cols[1].parse().unwrap_or(0.0),
            delta_mb: cols[2].parse().unwrap_or(0.0),
            saved_pct: cols[3].parse().unwrap_or(0.0),
            total_chunks: cols[4].parse().unwrap_or(0),
            reused_chunks: cols[5].parse().unwrap_or(0),
            fetched_chunks: cols[6].parse().unwrap_or(0),
        });
    }

    Ok(rows)
}

#[derive(Debug, Clone)]
struct CacheInfo {
    path: PathBuf,
    entries: usize,
    total_size_mb: f64,
}

fn load_cache_info(cache_path: &Option<PathBuf>) -> Option<CacheInfo> {
    let path = cache_path.clone().unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/ubuntu".into());
        PathBuf::from(home).join(".cache/daedalus")
    });

    if !path.exists() {
        return Some(CacheInfo {
            path,
            entries: 0,
            total_size_mb: 0.0,
        });
    }

    let mut entries = 0;
    let mut total_size: u64 = 0;

    if let Ok(walk) = std::fs::read_dir(&path) {
        for entry in walk.flatten() {
            if entry.path().is_dir() {
                entries += 1;
                if let Ok(size) = dir_size(&entry.path()) {
                    total_size += size;
                }
            }
        }
    }

    Some(CacheInfo {
        path,
        entries,
        total_size_mb: total_size as f64 / (1024.0 * 1024.0),
    })
}

fn dir_size(path: &PathBuf) -> std::io::Result<u64> {
    let mut size = 0;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_file() {
                if let Ok(metadata) = entry.metadata() {
                    size += metadata.len();
                }
            } else if p.is_dir() {
                if let Ok(sub) = dir_size(&p) {
                    size += sub;
                }
            }
        }
    }
    Ok(size)
}

fn ui(frame: &mut Frame, rows: &[BenchmarkRow], cache_info: &Option<CacheInfo>) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .spacing(1)
        .split(area);

    let left = chunks[0];
    let right = chunks[1];

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(5)])
        .split(left);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(right);

    // ---- Benchmark table ----
    let header_style = Style::new().add_modifier(Modifier::REVERSED);
    let header = Row::new(vec![
        Line::raw("Model"),
        Line::raw("Full(MB)"),
        Line::raw("Delta(MB)"),
        Line::raw("Saved"),
        Line::raw("Chunks"),
        Line::raw("Reuse"),
    ])
    .style(header_style)
    .height(2);

    let bar_style = Style::new().add_modifier(Modifier::REVERSED);
    let table = Table::new(
        rows.iter().map(|r| {
            let chunk_bar = format_chunks_bar(r.reused_chunks, r.fetched_chunks);
            let reuse_pct = if r.total_chunks > 0 {
                (r.reused_chunks as f64 / r.total_chunks as f64) * 100.0
            } else {
                0.0
            };
            Row::new(vec![
                Cell::new(Line::raw(r.model.as_str())),
                Cell::new(Line::raw(format!("{:.1}", r.full_mb))),
                Cell::new(Line::raw(format!("{:.1}", r.delta_mb))),
                Cell::new(Line::raw(format!("{:.1}%", r.saved_pct))),
                Cell::new(Line::raw(chunk_bar)),
                Cell::new(Line::raw(format!("{:.0}%", reuse_pct))),
            ])
            .style(bar_style)
        }),
        [
            Constraint::Min(18),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(24),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(
        Block::bordered()
            .title(Title::from(Line::raw("SISR Benchmark Results").bold()))
            .border_type(BorderType::Rounded),
    );

    frame.render_widget(table, left_chunks[0]);

    // ---- Cache info ----
    let cache_text = match cache_info {
        Some(info) => format!(
            "Path: {}\nEntries: {} | Size: {:.1} MB",
            info.path.display(),
            info.entries,
            info.total_size_mb,
        ),
        None => "Cache: not configured".into(),
    };

    let cache_widget = Paragraph::new(Text::from(cache_text))
        .alignment(Alignment::Left)
        .block(
            Block::bordered()
                .title(Title::from(Line::raw("daedalus cache").bold()))
                .border_type(BorderType::Rounded),
        );

    frame.render_widget(cache_widget, left_chunks[1]);

    // ---- Savings bar chart ----
    let saved_lines = build_bar_lines("Bandwidth Saved (%)", rows, |r| r.saved_pct, Color::Cyan);
    frame.render_widget(saved_lines, right_chunks[0]);

    // ---- Reuse bar chart ----
    let reuse_lines = build_bar_lines(
        "Chunk Reuse (%)",
        rows,
        |r| {
            if r.total_chunks > 0 {
                (r.reused_chunks as f64 / r.total_chunks as f64) * 100.0
            } else {
                0.0
            }
        },
        Color::Green,
    );
    frame.render_widget(reuse_lines, right_chunks[1]);
}

fn build_bar_lines<F>(
    title: &'static str,
    rows: &[BenchmarkRow],
    value_fn: F,
    color: Color,
) -> Paragraph<'static>
where
    F: Fn(&BenchmarkRow) -> f64,
{
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::raw(title));
    lines.push(Line::raw(""));

    let axis_label = format!("{:6}", "100%");
    lines.push(Line::raw(format!("+{}-{}", axis_label, " ".repeat(22))));

    for row in rows {
        let val = value_fn(row);
        #[allow(clippy::cast_sign_loss)] // bar_len is clamped to [0, 20], always non-negative
        let bar_len = (val / 100.0 * 20.0).round().clamp(0.0, 20.0) as usize;
        let bar_str = format!(
            "  {:>6.0}% |{}{}|",
            val,
            bar_sym::FULL.repeat(bar_len),
            " ".repeat(20 - bar_len),
        );
        lines.push(Line::raw(bar_str).style(Style::new().fg(color)));
        lines.push(Line::raw(format!("  {} ({:.0}%)", row.model, val)));
    }

    Paragraph::new(Text::from(lines)).block(
        Block::bordered()
            .title(Title::from(Line::raw(title).bold()))
            .border_type(BorderType::Rounded),
    )
}

fn format_chunks_bar(reused: u64, fetched: u64) -> String {
    let total = reused + fetched;
    if total == 0 {
        return "— (0)".to_string();
    }
    let bar_width = 8;
    #[allow(clippy::cast_sign_loss)] // value is clamped via .max(0.0) above
    let reused_len = ((reused as f64 / total as f64) * bar_width as f64)
        .round()
        .max(0.0) as usize;
    let mut s = String::new();
    for i in 0..bar_width {
        if i < reused_len {
            s.push_str(bar_sym::FULL);
        } else {
            s.push(' ');
        }
    }
    format!("{s}  ({reused}/{total})")
}
