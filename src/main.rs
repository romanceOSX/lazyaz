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
use api::mock::MockClient;
use api::WorkItemClient;
use app::{App, EditorRequest};
use auth::mock::MockAuthenticator;
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
    let (client, auth, backend_note) = build_backends(&cfg, want_login);
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

/// Choose the backend: PAT (env) → OAuth (cached/`--login`) → offline mock.
fn build_backends(
    cfg: &Config,
    want_login: bool,
) -> (Box<dyn WorkItemClient>, Box<dyn Authenticator>, String) {
    // 1. Personal Access Token from the environment.
    if let Ok(pat) = std::env::var("AZURE_DEVOPS_PAT")
        && !pat.trim().is_empty() && cfg.is_complete() {
            let account = std::env::var("AZURE_DEVOPS_ACCOUNT").unwrap_or_else(|_| "PAT".into());
            let state = AuthState { account, token: pat, scheme: AuthScheme::BasicPat };
            let client = AzureClient::new(cfg.org_url.clone(), cfg.project.clone(), state.header());
            return (
                Box::new(client),
                Box::new(PatAuthenticator::new(state)),
                "Azure DevOps (PAT)".into(),
            );
        }

    // 2. Entra ID OAuth: use a cached token, or sign in when asked.
    let mut oauth = OAuthAuthenticator::new();
    if oauth.token().is_none() && want_login
        && let Err(e) = oauth.login() {
            eprintln!("sign-in failed ({e}); starting in offline mode");
        }
    if let Some(token) = oauth.token()
        && cfg.is_complete() {
            let header = format!("Bearer {token}");
            let client = AzureClient::new(cfg.org_url.clone(), cfg.project.clone(), header);
            return (
                Box::new(client),
                Box::new(oauth),
                "Azure DevOps (OAuth)".into(),
            );
        }

    // 3. Offline: mock data so the app is usable without an account.
    (
        Box::new(MockClient::new()),
        Box::new(MockAuthenticator::default()),
        "offline demo — mock data (set AZURE_DEVOPS_PAT or run with --login)".into(),
    )
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
    Access Token, or pass --login for an interactive Entra ID sign-in (the token
    is cached). With neither, lazyaz runs offline against mock data.

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

        // Short poll keeps the conflict pulse animating and drives periodic refresh.
        if cevent::poll(Duration::from_millis(200))?
            && let Event::Key(key) = cevent::read()?
                && key.kind == cevent::KeyEventKind::Press {
                    event::handle_key(app, key);
                }
        app.poll();
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
        EditorRequest::Comment { .. } => String::new(),
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
