pub mod account;
pub mod cli;
pub mod config;
pub mod logger;
pub mod proxy;
pub mod upstream;

use account::manager::AccountManager;
use account::storage::{AccountStorage, AntigravityAccount};
use clap::{CommandFactory, Parser};
use cli::{AccountSubcommands, Cli, Commands, ConfigSubcommands};
use colored::Colorize;
use comfy_table::{presets::UTF8_FULL, Cell, Color, Row, Table};
use config::AppConfig;
use proxy::server::build_router;
use reqwest::Client;
use std::net::SocketAddr;
use std::sync::Arc;
use upstream::headers::generate_fingerprint;
use upstream::oauth::{exchange_code, generate_auth_url, get_project_id, get_user_email, refresh_access_token};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let cmd = match cli.command {
        Some(cmd) => cmd,
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
            return Ok(());
        }
    };

    match cmd {
        Commands::Serve(args) => {
            let broadcaster = logger::init_logger();
            let mut cfg = AppConfig::load_or_create(&args.config);

            if args.host != "127.0.0.1" {
                cfg.server.host = args.host.clone();
            }
            if args.port != 8317 {
                cfg.server.port = args.port;
            }
            if args.accounts.to_string_lossy() != "antigravity-accounts.json" {
                cfg.server.accounts_file = args.accounts.to_string_lossy().to_string();
            }

            let mut client_builder = Client::builder()
                .pool_max_idle_per_host(10)
                .tcp_keepalive(std::time::Duration::from_secs(60));

            if let Some(ref proxy_url) = cfg.server.proxy {
                if !proxy_url.trim().is_empty() {
                    let proxy = reqwest::Proxy::all(proxy_url)?;
                    client_builder = client_builder.proxy(proxy);
                }
            }

            let client = client_builder.build()?;

            let config_arc = Arc::new(cfg);
            let storage = AccountStorage::new(&config_arc.server.accounts_file);
            let account_manager = Arc::new(AccountManager::new(storage, config_arc.clone(), client.clone()));

            let router = build_router(account_manager.clone(), config_arc.clone(), client, broadcaster);

            let addr: SocketAddr = format!("{}:{}", config_arc.server.host, config_arc.server.port).parse()?;
            let listener = tokio::net::TcpListener::bind(addr).await?;

            println!("{}", "=========================================================".bright_cyan());
            println!("  {}", "🚀 ANTIGRAVITY PROXY (Rust Core Edition v0.8.0)".bold().bright_green());
            println!("{}", "=========================================================".bright_cyan());
            println!("  {} http://{}/v1", "• OpenAI API Endpoint: ".bright_yellow(), addr);
            println!("  {} http://{}/v1beta", "• Gemini API Endpoint: ".bright_yellow(), addr);
            println!("  {} http://{}", "• Web Management UI:   ".bright_yellow(), addr);
            println!("  {} {}", "• Accounts File:       ".bright_yellow(), config_arc.server.accounts_file.cyan());
            println!("  {} {}", "• Config File:         ".bright_yellow(), args.config.display().to_string().cyan());
            if let Some(ref p) = config_arc.server.proxy {
                println!("  {} {}", "• Outbound Proxy:      ".bright_yellow(), p.magenta());
            }
            println!("  {} {}", "• Active Strategy:     ".bright_yellow(), config_arc.rotation.strategy.cyan());
            println!("{}", "=========================================================".bright_cyan());

            axum::serve(listener, router).await?;
        }

        Commands::Account(args) => {
            let client = Client::new();
            let storage = AccountStorage::new(&args.file);

            match args.action {
                AccountSubcommands::List { json } => {
                    let accounts = storage.load_accounts();
                    if json {
                        println!("{}", serde_json::to_string_pretty(&accounts)?);
                        return Ok(());
                    }

                    if accounts.is_empty() {
                        println!("{}", "No accounts configured yet. Use 'account add' to add one.".yellow());
                        return Ok(());
                    }

                    let mut table = Table::new();
                    table.load_preset(UTF8_FULL);
                    table.set_header(vec![
                        Cell::new("Email").fg(Color::Cyan),
                        Cell::new("Health").fg(Color::Green),
                        Cell::new("Project ID").fg(Color::Yellow),
                        Cell::new("Cool Down").fg(Color::Magenta),
                        Cell::new("Quotas Left").fg(Color::Blue),
                    ]);

                    let now = chrono::Utc::now().timestamp_millis();

                    for acc in accounts {
                        let health_color = if acc.health_score > 80.0 {
                            Color::Green
                        } else if acc.health_score > 50.0 {
                            Color::Yellow
                        } else {
                            Color::Red
                        };

                        let cd_str = if acc.cooldowns.is_empty() {
                            "Ready".to_string()
                        } else {
                            acc.cooldowns
                                .iter()
                                .filter(|(_, exp)| **exp > now)
                                .map(|(k, exp)| format!("{}: {}s", k.split('|').last().unwrap_or(""), (*exp - now) / 1000))
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        let cd_display = if cd_str.is_empty() { "Ready".to_string() } else { cd_str };

                        let quota_summary = acc
                            .quota
                            .map(|qs| {
                                qs.iter()
                                    .take(3)
                                    .map(|q| format!("{}: {:.0}%", q.group_name, q.get_percentage()))
                                    .collect::<Vec<_>>()
                                    .join(" | ")
                            })
                            .unwrap_or_else(|| "N/A".to_string());

                        table.add_row(Row::from(vec![
                            Cell::new(&acc.email),
                            Cell::new(format!("{:.0}", acc.health_score)).fg(health_color),
                            Cell::new(acc.project_id.as_deref().unwrap_or("N/A")),
                            Cell::new(cd_display),
                            Cell::new(quota_summary),
                        ]));
                    }

                    println!("{table}");
                }

                AccountSubcommands::Add { token, oauth } => {
                    if let Some(refresh_token) = token {
                        println!("{}", "Exchanging refresh token for access token...".cyan());
                        let token_res = refresh_access_token(&client, &refresh_token).await?;
                        let email = get_user_email(&client, &token_res.access_token).await?;
                        let fp = generate_fingerprint(Some(&email));
                        let project_id = get_project_id(&client, &token_res.access_token, &fp).await?;

                        let new_account = AntigravityAccount {
                            email: email.clone(),
                            refresh_token,
                            access_token: Some(token_res.access_token),
                            expires_at: Some(chrono::Utc::now().timestamp_millis() + (token_res.expires_in * 1000)),
                            project_id: if project_id.is_empty() { None } else { Some(project_id) },
                            health_score: 100.0,
                            last_used: 0,
                            token_usage: 0,
                            cooldowns: std::collections::HashMap::new(),
                            fingerprint: Some(fp),
                            quota: None,
                        };

                        let mut accounts = storage.load_accounts();
                        accounts.retain(|a| a.email != email);
                        accounts.push(new_account);
                        storage.save_accounts(&accounts)?;

                        println!("{} Account added: {}", "✔".green(), email.bold());
                    } else if oauth {
                        let auth_port = 8317;
                        let auth_url = generate_auth_url(auth_port);

                        println!("{}", "Starting temporary local OAuth listener on port 8317...".cyan());
                        println!("Please authorize Antigravity in your browser:");
                        println!("  {}", auth_url.bright_blue().underline());

                        let _ = open::that(&auth_url);

                        use axum::{extract::Query, routing::get, Router};
                        use std::sync::mpsc;

                        let (tx, rx) = mpsc::channel::<String>();
                        let tx_arc = Arc::new(std::sync::Mutex::new(Some(tx)));

                        let app = Router::new().route(
                            "/oauth-callback",
                            get({
                                let tx_clone = tx_arc.clone();
                                move |Query(params): Query<std::collections::HashMap<String, String>>| async move {
                                    if let Some(code) = params.get("code") {
                                        if let Some(tx) = tx_clone.lock().unwrap().take() {
                                            let _ = tx.send(code.clone());
                                        }
                                        "<h1>Authorization Successful!</h1><p>You can close this tab and return to the terminal.</p>"
                                    } else {
                                        "<h1>Authorization Failed!</h1>"
                                    }
                                }
                            }),
                        );

                        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", auth_port)).await?;
                        let server_handle = tokio::spawn(async move {
                            let _ = axum::serve(listener, app).await;
                        });

                        println!("{}", "Waiting for browser OAuth callback...".yellow());
                        let code = rx.recv()?;
                        server_handle.abort();

                        let redirect_uri = format!("http://localhost:{}/oauth-callback", auth_port);
                        let token_res = exchange_code(&client, &code, &redirect_uri).await?;
                        let refresh_token = token_res.refresh_token.ok_or("No refresh token returned by Google")?;
                        let email = get_user_email(&client, &token_res.access_token).await?;
                        let fp = generate_fingerprint(Some(&email));
                        let project_id = get_project_id(&client, &token_res.access_token, &fp).await?;

                        let new_account = AntigravityAccount {
                            email: email.clone(),
                            refresh_token,
                            access_token: Some(token_res.access_token),
                            expires_at: Some(chrono::Utc::now().timestamp_millis() + (token_res.expires_in * 1000)),
                            project_id: if project_id.is_empty() { None } else { Some(project_id) },
                            health_score: 100.0,
                            last_used: 0,
                            token_usage: 0,
                            cooldowns: std::collections::HashMap::new(),
                            fingerprint: Some(fp),
                            quota: None,
                        };

                        let mut accounts = storage.load_accounts();
                        accounts.retain(|a| a.email != email);
                        accounts.push(new_account);
                        storage.save_accounts(&accounts)?;

                        println!("{} OAuth login successful! Added account: {}", "✔".green(), email.bold());
                    } else {
                        println!("Please provide either --oauth or --token <REFRESH_TOKEN>");
                    }
                }

                AccountSubcommands::Refresh { email } => {
                    let mut accounts = storage.load_accounts();
                    for acc in accounts.iter_mut() {
                        if let Some(ref target) = email {
                            if acc.email != *target {
                                continue;
                            }
                        }

                        println!("Refreshing tokens and quota for {}...", acc.email.cyan());
                        if let Ok(token_res) = refresh_access_token(&client, &acc.refresh_token).await {
                            acc.access_token = Some(token_res.access_token.clone());
                            acc.expires_at = Some(chrono::Utc::now().timestamp_millis() + (token_res.expires_in * 1000));

                            let fp = acc.fingerprint.clone().unwrap_or_else(|| generate_fingerprint(Some(&acc.email)));
                            if let Some(ref pid) = acc.project_id {
                                match upstream::quota::fetch_quota(&client, &token_res.access_token, &fp, pid).await {
                                    Ok(quotas) => {
                                        println!("Fetched {} quotas", quotas.len());
                                        acc.quota = Some(quotas);
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to fetch quota: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    if let Err(e) = storage.save_accounts(&accounts) {
                        eprintln!("Failed to save accounts: {}", e);
                    }
                    println!("{} Quotas and tokens refreshed.", "✔".green());
                }

                AccountSubcommands::Remove { email } => {
                    let mut accounts = storage.load_accounts();
                    let len_before = accounts.len();
                    accounts.retain(|a| a.email != email);
                    if accounts.len() < len_before {
                        storage.save_accounts(&accounts)?;
                        println!("{} Removed account {}", "✔".green(), email.cyan());
                    } else {
                        println!("Account {} not found.", email.yellow());
                    }
                }

                AccountSubcommands::Reset { email } => {
                    let mut accounts = storage.load_accounts();
                    for acc in accounts.iter_mut() {
                        if let Some(ref target) = email {
                            if acc.email != *target {
                                continue;
                            }
                        }
                        acc.cooldowns.clear();
                        acc.health_score = 100.0;
                    }
                    storage.save_accounts(&accounts)?;
                    println!("{} Reset cooldowns and health score.", "✔".green());
                }
            }
        }

        Commands::Config(args) => match args.action {
            ConfigSubcommands::Show => {
                let cfg = AppConfig::load_or_create(&args.file);
                println!("{}", toml::to_string_pretty(&cfg)?);
            }
            ConfigSubcommands::Set { key, value } => {
                println!("Config key: {}, value: {}", key, value);
            }
        },

        Commands::Test(args) => {
            println!("Testing proxy endpoint: {}", args.url.cyan());
            println!("Model: {}", args.model.green());

            let client = Client::new();
            let start = std::time::Instant::now();

            let payload = serde_json::json!({
                "model": args.model,
                "messages": [
                    { "role": "user", "content": "Hello, please reply with 'Proxy works perfectly!'" }
                ],
                "stream": false
            });

            match client.post(&args.url).json(&payload).send().await {
                Ok(res) => {
                    let elapsed = start.elapsed();
                    let status = res.status();
                    let body = res.text().await.unwrap_or_default();
                    if status.is_success() {
                        println!("{} Response ({:?}):\n{}", "✔ Status 200 OK".green().bold(), elapsed, body);
                    } else {
                        println!("{} Response: {}", format!("✖ Status {}", status).red().bold(), body);
                    }
                }
                Err(e) => {
                    println!("{} Error: {}", "✖ Failed to connect".red().bold(), e);
                }
            }
        }
    }

    Ok(())
}
