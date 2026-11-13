use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "antigravity-proxy",
    author = "AntigravityProxy Contributors",
    version = "0.8.0",
    about = "🚀 高性能 Antigravity & Google Cloud Code 内部 API 反代与 CLI 控制中心",
    after_help = r#"
常用命令示例 (EXAMPLES):
  1. 启动服务 (同时启动 Web 仪表盘与 OpenAI 兼容端点):
     antigravity-proxy serve --port 8317
     antigravity-proxy serve --port 8318 --host 0.0.0.0

  2. 账号与配额管理:
     antigravity-proxy account list                    # 表格化查看所有已绑定账号的健康度与配额
     antigravity-proxy account list --json             # 输出纯 JSON 格式
     antigravity-proxy account add --oauth             # 启动本地回调并唤起浏览器完成 Google 授权
     antigravity-proxy account add --token <REFRESH>   # 通过已有 Refresh Token 导入账号
     antigravity-proxy account refresh [email]         # 立即向 Google 查询并刷新最新模型配额
     antigravity-proxy account reset                   # 重置所有账号的 429 冷却状态与健康分
     antigravity-proxy account remove <email>          # 移除指定账号

  3. 配置文件管理:
     antigravity-proxy config show                     # 打印当前 config.toml 内容

  4. 线上可用模型与配额查看:
     antigravity-proxy models                          # 表格化拉取当前上游最新可用模型及配额
     antigravity-proxy models --json                   # 输出纯 JSON 格式

  5. 连通性与模型自检:
     antigravity-proxy test --model gemini-4-argon-high
     antigravity-proxy test --model gemini-4-hydrogen-high
"#
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the proxy server and embedded web management console
    Serve(ServeArgs),

    /// Account management (list, add, remove, refresh, reset)
    Account(AccountArgs),

    /// Fetch and display real-time available models and quotas from Google upstream
    Models(ModelsArgs),

    /// View or update configuration
    Config(ConfigArgs),

    /// Test end-to-end proxy connectivity and model streaming
    Test(TestArgs),
}

#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Bind host address
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Bind port
    #[arg(short, long, default_value_t = 8317)]
    pub port: u16,

    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    pub config: PathBuf,

    /// Path to accounts JSON file
    #[arg(short, long, default_value = "antigravity-accounts.json")]
    pub accounts: PathBuf,
}

#[derive(Args, Debug)]
pub struct AccountArgs {
    #[command(subcommand)]
    pub action: AccountSubcommands,

    /// Path to accounts JSON file
    #[arg(short, long, default_value = "antigravity-accounts.json")]
    pub file: PathBuf,

    /// Path to config file (for outbound proxy settings)
    #[arg(short, long, default_value = "config.toml")]
    pub config: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum AccountSubcommands {
    /// List all configured accounts with quotas and health status
    List {
        /// Output raw JSON instead of table
        #[arg(long)]
        json: bool,
    },

    /// Add a new account via OAuth or direct refresh token
    Add {
        /// Direct Google OAuth refresh token
        #[arg(long)]
        token: Option<String>,

        /// Launch browser for interactive Google OAuth login
        #[arg(long)]
        oauth: bool,
    },

    /// Refresh token and fetch latest quotas for accounts
    Refresh {
        /// Optional specific account email
        email: Option<String>,
    },

    /// Remove an account by email
    Remove {
        /// Account email to remove
        email: String,
    },

    /// Clear cooldowns and reset health score for accounts
    Reset {
        /// Optional specific account email to reset (resets all if omitted)
        email: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct ModelsArgs {
    /// Path to accounts JSON file
    #[arg(short, long, default_value = "antigravity-accounts.json")]
    pub file: PathBuf,

    /// Path to config file (for outbound proxy settings)
    #[arg(short, long, default_value = "config.toml")]
    pub config: PathBuf,

    /// Filter by specific account email
    #[arg(short, long)]
    pub email: Option<String>,

    /// Output raw JSON instead of table
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigSubcommands,

    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    pub file: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum ConfigSubcommands {
    /// Display current configuration
    Show,

    /// Set a configuration value (e.g. server.port 8317)
    Set {
        key: String,
        value: String,
    },
}

#[derive(Args, Debug)]
pub struct TestArgs {
    /// Target model to test
    #[arg(short, long, default_value = "gemini-3.8-flash-high")]
    pub model: String,

    /// Target endpoint URL
    #[arg(short, long, default_value = "http://127.0.0.1:8317/v1/chat/completions")]
    pub url: String,
}
