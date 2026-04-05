use std::{
    io::{Stdout, Write, stdout},
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use atai_core::{
    llm::{GenerationResult, LlmClient},
    policy::{PolicyEngine, RiskLevel, RiskReport},
    review::ReviewSnapshot,
};
use crossterm::{
    cursor::{Hide, MoveToColumn, MoveUp, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{
        Attribute, Color, Print, PrintStyledContent, ResetColor, SetAttribute, SetForegroundColor,
        Stylize,
    },
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use tokio::task::JoinHandle;

#[derive(Clone, Debug)]
pub enum SessionOutcome {
    Cancelled(Option<ReviewSnapshot>),
    Approved(ReviewSnapshot),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConfirmState {
    HighRiskArmed,
}

#[derive(Clone, Debug)]
enum PendingAction {
    Generate,
}

#[derive(Clone, Debug)]
enum StatusTone {
    Info,
    Success,
    Warning,
    Error,
}

struct App {
    goal: String,
    generation: Option<GenerationResult>,
    risk_report: Option<RiskReport>,
    status_icon: String,
    status_message: String,
    status_tone: StatusTone,
    confirm_state: Option<ConfirmState>,
    pending_action: Option<PendingAction>,
    generation_started_at: Option<Instant>,
    generation_elapsed: Option<Duration>,
    generation_task: Option<JoinHandle<Result<GenerationResult>>>,
}

impl App {
    fn new(goal: String) -> Self {
        Self {
            goal,
            generation: None,
            risk_report: None,
            status_icon: "🤔".to_string(),
            status_message: "thinking...".to_string(),
            status_tone: StatusTone::Info,
            confirm_state: None,
            pending_action: Some(PendingAction::Generate),
            generation_started_at: None,
            generation_elapsed: None,
            generation_task: None,
        }
    }

    fn snapshot(&self) -> Option<ReviewSnapshot> {
        let generated = self.generation.clone()?;
        let risk = self.risk_report.clone()?;
        Some(ReviewSnapshot {
            command: generated.command,
            summary: generated.summary,
            assumptions: generated.assumptions,
            risk_hints: generated.risk_hints,
            risk_level: risk.level,
            risk_reasons: risk.reasons,
            feedback_history: Vec::new(),
        })
    }

    fn set_status(&mut self, icon: &str, text: &str, tone: StatusTone) {
        self.status_icon = icon.to_string();
        self.status_message = text.to_string();
        self.status_tone = tone;
    }

    fn status_line(&self) -> String {
        let (elapsed_text, in_progress) = self.formatted_elapsed();
        if in_progress {
            format!(
                "{} {} {}...",
                self.status_icon, self.status_message, elapsed_text
            )
        } else {
            format!(
                "{} {} {}",
                self.status_icon, self.status_message, elapsed_text
            )
        }
    }

    fn formatted_elapsed(&self) -> (String, bool) {
        if let Some(started_at) = self.generation_started_at {
            let seconds = started_at.elapsed().as_secs().saturating_add(1);
            return (format!("{seconds}s"), true);
        }

        let seconds = self
            .generation_elapsed
            .map(|elapsed| elapsed.as_secs().max(1))
            .unwrap_or(0);
        (format!("{seconds}s"), false)
    }

    async fn run(
        &mut self,
        terminal: &mut InlineTerminal,
        llm_client: &LlmClient,
        policy_engine: &PolicyEngine,
        cwd: &Path,
    ) -> Result<SessionOutcome> {
        loop {
            if let Some(action) = self.pending_action.take() {
                self.start_pending_action(action, llm_client);
            }

            self.complete_generation_if_ready(policy_engine, cwd)
                .await?;
            terminal.render(self)?;

            if event::poll(Duration::from_millis(120)).context("Failed to read terminal events")? {
                let Event::Key(key_event) =
                    event::read().context("Failed to read keyboard input")?
                else {
                    continue;
                };

                if key_event.kind != KeyEventKind::Press {
                    continue;
                }

                if let Some(outcome) = self.handle_key_event(key_event) {
                    terminal.render(self)?;
                    return Ok(outcome);
                }
            }
        }
    }

    fn start_pending_action(&mut self, action: PendingAction, llm_client: &LlmClient) {
        match action {
            PendingAction::Generate => {
                self.set_status("🤔", "thinking...", StatusTone::Info);
                self.confirm_state = None;
                self.generation = None;
                self.risk_report = None;
                self.generation_started_at = Some(Instant::now());
                self.generation_elapsed = None;

                let llm_client = llm_client.clone();
                let goal = self.goal.clone();
                self.generation_task = Some(tokio::spawn(async move {
                    llm_client.generate_command(&goal, &[]).await
                }));
            }
        }
    }

    async fn complete_generation_if_ready(
        &mut self,
        policy_engine: &PolicyEngine,
        cwd: &Path,
    ) -> Result<()> {
        let is_ready = self
            .generation_task
            .as_ref()
            .map(JoinHandle::is_finished)
            .unwrap_or(false);

        if !is_ready {
            return Ok(());
        }

        let started_at = self.generation_started_at.take();
        self.generation_elapsed = started_at.map(|started| started.elapsed());

        let task = self
            .generation_task
            .take()
            .context("Missing generation task handle")?;

        match task
            .await
            .context("The generation task failed to complete")?
        {
            Ok(generated) => {
                let risk = policy_engine.classify(&generated.command, cwd);
                match risk.level {
                    RiskLevel::Safe => {
                        self.set_status("✅", "command ready", StatusTone::Success);
                    }
                    RiskLevel::HighRisk => {
                        self.set_status("⚠️", "high-risk command ready", StatusTone::Warning);
                    }
                    RiskLevel::Deny => {
                        self.set_status("⛔", "command blocked by policy", StatusTone::Error);
                    }
                }
                self.confirm_state = None;
                self.generation = Some(generated);
                self.risk_report = Some(risk);
            }
            Err(error) => {
                self.set_status(
                    "⛔",
                    &format!("generation failed: {error:#}"),
                    StatusTone::Error,
                );
            }
        }

        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Option<SessionOutcome> {
        if key_event.modifiers.contains(KeyModifiers::CONTROL) {
            match key_event.code {
                KeyCode::Char('c') | KeyCode::Char('q') => {
                    return Some(SessionOutcome::Cancelled(self.snapshot()));
                }
                KeyCode::Char('r') => {
                    self.pending_action = Some(PendingAction::Generate);
                    self.set_status("🤔", "regenerating...", StatusTone::Info);
                    return None;
                }
                KeyCode::Char('e') => return self.begin_execution_or_close(),
                _ => {}
            }
        }

        match key_event.code {
            KeyCode::Enter => self.begin_execution_or_close(),
            KeyCode::Esc => {
                self.confirm_state = None;
                self.set_status("ℹ️", "confirmation cancelled", StatusTone::Info);
                None
            }
            _ => None,
        }
    }

    fn begin_execution_or_close(&mut self) -> Option<SessionOutcome> {
        if self.generation.is_none() {
            return Some(SessionOutcome::Cancelled(self.snapshot()));
        }
        self.begin_execution()
    }

    fn begin_execution(&mut self) -> Option<SessionOutcome> {
        let Some(risk_report) = &self.risk_report else {
            self.set_status("ℹ️", "no command available yet", StatusTone::Info);
            return None;
        };

        match risk_report.level {
            RiskLevel::Safe => self.snapshot().map(SessionOutcome::Approved),
            RiskLevel::HighRisk => {
                if self.confirm_state == Some(ConfirmState::HighRiskArmed) {
                    self.snapshot().map(SessionOutcome::Approved)
                } else {
                    self.confirm_state = Some(ConfirmState::HighRiskArmed);
                    self.set_status(
                        "⚠️",
                        "press Enter again to execute the high-risk command",
                        StatusTone::Warning,
                    );
                    None
                }
            }
            RiskLevel::Deny => {
                self.set_status(
                    "⛔",
                    "the current command cannot be executed",
                    StatusTone::Error,
                );
                None
            }
        }
    }

    fn status_color(&self) -> Color {
        match self.status_tone {
            StatusTone::Info => Color::Cyan,
            StatusTone::Success => Color::Green,
            StatusTone::Warning => Color::Yellow,
            StatusTone::Error => Color::Red,
        }
    }

    fn hint_line(&self) -> &'static str {
        match (
            self.risk_report.as_ref().map(|risk| &risk.level),
            self.confirm_state.as_ref(),
        ) {
            (Some(RiskLevel::Deny), _) => "Enter close  Ctrl+R regenerate  Ctrl+C/Ctrl+Q quit",
            (Some(RiskLevel::HighRisk), Some(ConfirmState::HighRiskArmed)) => {
                "Enter execute  Esc cancel  Ctrl+C/Ctrl+Q quit"
            }
            (Some(RiskLevel::HighRisk), None) => "Enter arm  Ctrl+R regenerate  Ctrl+C/Ctrl+Q quit",
            (Some(RiskLevel::Safe), _) => "Enter execute  Ctrl+R regenerate  Ctrl+C/Ctrl+Q quit",
            _ => "Enter close  Ctrl+C/Ctrl+Q quit",
        }
    }

    fn rendered_line_count(&self) -> u16 {
        let mut lines = 3;

        if let Some(risk) = &self.risk_report {
            lines += 4;
            if !risk.reasons.is_empty() {
                lines += 1;
            }
        }

        lines
    }

    fn render_signature(&self) -> String {
        let mut parts = vec![format!(
            "status={:?}|{}",
            self.status_tone,
            self.status_line()
        )];

        if let Some(generated) = &self.generation {
            parts.push(format!("command={}", generated.command));
        }

        if let Some(risk) = &self.risk_report {
            parts.push(format!("risk={:?}", risk.level));
            if let Some(reason) = risk.reasons.first() {
                parts.push(format!("reason={reason}"));
            }
        }

        parts.push(format!("hint={}", self.hint_line()));
        parts.join("\n")
    }
}

struct InlineTerminal {
    stdout: Stdout,
    last_render_signature: Option<String>,
    last_rendered_lines: u16,
}

impl InlineTerminal {
    fn new() -> Result<Self> {
        let mut stdout = stdout();
        enable_raw_mode().context("Failed to enable raw mode")?;
        execute!(stdout, Hide).context("Failed to initialize inline terminal")?;
        Ok(Self {
            stdout,
            last_render_signature: None,
            last_rendered_lines: 0,
        })
    }

    fn render(&mut self, app: &App) -> Result<()> {
        let signature = app.render_signature();
        if self.last_render_signature.as_deref() == Some(signature.as_str()) {
            return Ok(());
        }

        queue!(self.stdout, MoveToColumn(0)).context("Failed to move cursor to column 0")?;
        if self.last_rendered_lines > 0 {
            queue!(self.stdout, MoveUp(self.last_rendered_lines))
                .context("Failed to move cursor to the previous inline region")?;
        }
        queue!(self.stdout, Clear(ClearType::FromCursorDown))
            .context("Failed to reset inline terminal region")?;

        self.print_line(app.status_color(), true, &app.status_line())?;
        self.print_blank_line()?;

        if let Some(generated) = &app.generation {
            self.print_labeled_line("command: ", &format!("\"{}\"", generated.command))?;
            self.print_blank_line()?;

            if let Some(risk) = &app.risk_report {
                self.print_labeled_line("risk: ", &risk.level.to_string())?;

                if let Some(reason) = risk.reasons.first() {
                    self.print_dim_line(&format!("reason: {reason}"))?;
                }
            }

            self.print_blank_line()?;
        }

        self.print_hint_line(app.hint_line())?;
        self.stdout
            .flush()
            .context("Failed to flush terminal output")?;
        self.last_render_signature = Some(signature);
        self.last_rendered_lines = app.rendered_line_count();
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        execute!(
            self.stdout,
            ResetColor,
            SetAttribute(Attribute::Reset),
            Show,
            Print("\r\n")
        )
        .context("Failed to restore terminal output")?;
        disable_raw_mode().context("Failed to disable raw mode")?;
        Ok(())
    }

    fn print_line(&mut self, color: Color, bold: bool, text: &str) -> Result<()> {
        queue!(self.stdout, SetForegroundColor(color)).context("Failed to set line color")?;
        if bold {
            queue!(self.stdout, SetAttribute(Attribute::Bold))
                .context("Failed to set bold attribute")?;
        }
        queue!(
            self.stdout,
            Print(text),
            ResetColor,
            SetAttribute(Attribute::Reset)
        )
        .context("Failed to print line")?;
        queue!(self.stdout, Print("\r\n")).context("Failed to print newline")?;
        Ok(())
    }

    fn print_labeled_line(&mut self, label: &str, value: &str) -> Result<()> {
        queue!(
            self.stdout,
            PrintStyledContent(label.with(Color::Blue).attribute(Attribute::Bold)),
            PrintStyledContent(value.with(Color::White))
        )
        .context("Failed to print labeled line")?;
        queue!(self.stdout, Print("\r\n")).context("Failed to print newline")?;
        Ok(())
    }

    fn print_dim_line(&mut self, text: &str) -> Result<()> {
        queue!(
            self.stdout,
            PrintStyledContent(text.with(Color::DarkGrey).attribute(Attribute::Dim))
        )
        .context("Failed to print dim line")?;
        queue!(self.stdout, Print("\r\n")).context("Failed to print newline")?;
        Ok(())
    }

    fn print_blank_line(&mut self) -> Result<()> {
        queue!(self.stdout, Print("\r\n")).context("Failed to print blank line")?;
        Ok(())
    }

    fn print_hint_line(&mut self, text: &str) -> Result<()> {
        self.print_dim_line(text)
    }
}

pub async fn run_review_session(
    goal: &str,
    llm_client: &LlmClient,
    policy_engine: &PolicyEngine,
    cwd: &Path,
) -> Result<SessionOutcome> {
    let mut terminal = InlineTerminal::new()?;
    let mut app = App::new(goal.to_string());
    let result = app.run(&mut terminal, llm_client, policy_engine, cwd).await;
    let restore_result = terminal.finish();
    match (result, restore_result) {
        (Ok(outcome), Ok(())) => Ok(outcome),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(_restore_error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{App, ConfirmState, StatusTone};
    use atai_core::{
        llm::GenerationResult,
        policy::{RiskLevel, RiskReport},
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn seed_app(level: RiskLevel) -> App {
        let mut app = App::new("test goal".to_string());
        app.generation = Some(GenerationResult {
            command: "echo ok".to_string(),
            summary: "Prints ok".to_string(),
            assumptions: Vec::new(),
            risk_hints: Vec::new(),
        });
        app.risk_report = Some(RiskReport {
            level,
            reasons: vec!["reason".to_string()],
        });
        app
    }

    #[test]
    fn enter_executes_safe_command() {
        let mut app = seed_app(RiskLevel::Safe);
        let outcome = app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(outcome.is_some());
    }

    #[test]
    fn enter_requires_double_confirm_for_high_risk_command() {
        let mut app = seed_app(RiskLevel::HighRisk);
        let first = app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(first.is_none());
        assert_eq!(app.confirm_state, Some(ConfirmState::HighRiskArmed));

        let second = app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(second.is_some());
    }

    #[test]
    fn enter_closes_when_no_command_is_available() {
        let mut app = App::new("test goal".to_string());
        let outcome = app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(outcome.is_some());
    }

    #[test]
    fn ctrl_r_schedules_regeneration() {
        let mut app = seed_app(RiskLevel::Safe);
        let _ = app.handle_key_event(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert!(app.pending_action.is_some());
    }

    #[test]
    fn ctrl_c_cancels_the_inline_view() {
        let mut app = seed_app(RiskLevel::Safe);
        let outcome =
            app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(outcome.is_some());
    }

    #[test]
    fn render_signature_changes_when_status_changes() {
        let mut app = seed_app(RiskLevel::Safe);
        let before = app.render_signature();

        app.set_status("ℹ️", "updated", StatusTone::Info);
        let after = app.render_signature();

        assert_ne!(before, after);
    }

    #[test]
    fn rendered_line_count_matches_thinking_state() {
        let app = App::new("test goal".to_string());
        assert_eq!(app.rendered_line_count(), 3);
    }

    #[test]
    fn rendered_line_count_matches_ready_state_with_reason() {
        let app = seed_app(RiskLevel::Safe);
        assert_eq!(app.rendered_line_count(), 8);
    }
}
