mod api;
mod app;
mod auth;
mod config;
mod event;
mod keys;
mod session;
mod ui;

use anyhow::{Context, Result};
use api::azure::AzureClient;
use api::WorkItemClient;
use app::{App, EditorRequest};
use auth::oauth::OAuthAuthenticator;
use auth::pat::PatAuthenticator;
use auth::{AuthScheme, AuthState, Authenticator};
use config::Config;
use crossterm::event::{self as cevent, Event};
use std::io::{Read, Write};
use std::time::Duration;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return Ok(());
    }
    // Force the first-run config wizard, even if a config already exists.
    let force_wizard = args
        .iter()
        .any(|a| a == "--setup" || a == "--reconfigure" || a == "setup");
    let want_login = args.iter().any(|a| a == "--login");

    let cfg = Config::load().unwrap_or_default();
    let (client, auth, backend_note) = build_backends(&cfg, want_login)?;
    let mut app = App::with_backends(cfg, client, auth);
    app.status = backend_note;
    if force_wizard {
        app.start_wizard();
    } else {
        app.restore_session();
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    app.save_session();
    result
}

/// A constructed work-item backend: client, authenticator, and a status note.
type Backend = (Box<dyn WorkItemClient>, Box<dyn Authenticator>, String);

/// Choose the backend: PAT (env) or Entra ID OAuth (cached token / device sign-in).
/// There is no offline fallback — lazyaz always talks to the real Azure DevOps API.
fn build_backends(cfg: &Config, want_login: bool) -> Result<Backend> {
    // 1. Personal Access Token from the environment.
    if let Ok(pat) = std::env::var("AZURE_DEVOPS_PAT")
        && !pat.trim().is_empty() && cfg.is_complete() {
            let account = std::env::var("AZURE_DEVOPS_ACCOUNT").unwrap_or_else(|_| "PAT".into());
            let state = AuthState { account, token: pat, scheme: AuthScheme::BasicPat };
            let client = AzureClient::new(
                cfg.org_url.clone(),
                cfg.project.clone(),
                cfg.team.clone(),
                state.header(),
            );
            return Ok((
                Box::new(client),
                Box::new(PatAuthenticator::new(state)),
                "Azure DevOps (PAT)".into(),
            ));
        }

    // 2. Entra ID OAuth: use a cached token, or sign in via the device-code
    //    flow. We build the real backend even before org/project are configured
    //    so the first-run wizard can list the user's actual orgs and projects.
    let mut oauth = OAuthAuthenticator::new();
    // Reuse a valid token, renew it silently via the refresh token, or sign in
    // interactively only as a last resort (or when --login is passed).
    oauth
        .ensure_token(want_login)
        .context("Entra ID sign-in failed")?;
    let token = oauth
        .token()
        .context("no Azure DevOps credentials available")?;
    let header = format!("Bearer {token}");
    let client = AzureClient::new(
        cfg.org_url.clone(),
        cfg.project.clone(),
        cfg.team.clone(),
        header,
    );
    Ok((
        Box::new(client),
        Box::new(oauth),
        "Azure DevOps (OAuth)".into(),
    ))
}

fn print_usage() {
    println!(
        "lazyaz — a lazygit-style TUI for Azure DevOps work items

USAGE:
    lazyaz [OPTIONS]

OPTIONS:
    --login                   Sign in to Azure DevOps via Entra ID (device code)
    --setup, --reconfigure    Run the first-run config wizard (pick org/project)
    -h, --help                Show this help

AUTH:
    Set AZURE_DEVOPS_PAT (and have org/project configured) to use a Personal
    Access Token. Otherwise lazyaz signs you in with Entra ID via the device-code
    flow (the token is cached) — a sign-in is required to reach Azure DevOps.

With no options, lazyaz opens normally and runs the setup wizard automatically
on first launch (when no config exists)."
    );
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, app))?;

        // Handle a pending $EDITOR request outside the draw closure.
        if let Some(req) = app.pending_editor.take() {
            run_editor(terminal, app, req)?;
            continue;
        }

        // Wait for input. While a conflict menu is open or a refresh is in
        // flight we tick fast so the pulsing border / loading spinner animate;
        // otherwise we idle on a long timeout to avoid needless redraws.
        let tick = if app.resolution.is_some()
            || app.is_loading()
            || app.is_pushing()
            || app.tree_loading()
        {
            Duration::from_millis(120)
        } else {
            Duration::from_secs(1)
        };
        if cevent::poll(tick)?
            && let Event::Key(key) = cevent::read()?
                && key.kind == cevent::KeyEventKind::Press {
                    event::handle_key(app, key);
                }
        app.poll();
        app.drain_refresh();
        app.live_poll();
        app.drain_live();
        app.drain_push();
        app.drain_tree();
    }
    Ok(())
}

/// Suspend the TUI, open `$EDITOR` on a temp file, restore, apply the result.
fn run_editor(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    req: EditorRequest,
) -> Result<()> {
    let initial = match &req {
        EditorRequest::Field { initial, .. } => initial.clone(),
        EditorRequest::EditComment { initial, .. } => initial.clone(),
        // Conflict merge: seed the editor with the conflict-marker text.
        EditorRequest::Merge { .. } => app.merge_seed.take().unwrap_or_default(),
        EditorRequest::Comment => String::new(),
    };

    let edited = with_suspended(terminal, || edit_in_editor(&initial))?;
    if let Some(text) = edited {
        app.apply_editor_result(req, text);
    } else {
        app.status = "edit cancelled".into();
    }
    Ok(())
}

/// Leave the alternate screen / raw mode, run `f`, then re-enter.
fn with_suspended<T>(
    terminal: &mut ratatui::DefaultTerminal,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    ratatui::restore();
    let out = f();
    *terminal = ratatui::init();
    let _ = terminal.clear();
    out
}

fn edit_in_editor(initial: &str) -> Result<Option<String>> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    let mut tmp = tempfile::Builder::new()
        .prefix("lazyaz-")
        .suffix(".md")
        .tempfile()
        .context("creating temp file")?;
    tmp.write_all(initial.as_bytes())?;
    tmp.flush()?;
    let path = tmp.path().to_path_buf();

    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .with_context(|| format!("launching editor '{editor}'"))?;
    if !status.success() {
        return Ok(None);
    }

    let mut text = String::new();
    std::fs::File::open(&path)?.read_to_string(&mut text)?;
    Ok(Some(text))
}
