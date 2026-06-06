use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "codex-helper",
    about = "Codex Helper - 轻量代理，让 Codex 使用 DeepSeek 等国产大模型",
    version
)]
pub struct Cli {
    /// 省略子命令时，Windows / macOS 默认执行 start（启动托盘与代理）
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 初始化配置并写入 Codex config.toml
    Init,
    /// 启动本地代理（Windows / macOS 默认显示菜单栏托盘）
    Start {
        #[arg(long, help = "不显示托盘，仅用命令行模式")]
        no_tray: bool,
    },
    /// 查看当前状态
    Status,
    /// 列出可用模型预设
    List,
    /// 切换到指定模型
    Use {
        provider: String,
    },
    /// 测试当前模型连通性
    Test,
    /// 一键诊断环境
    Doctor,
    /// 修复 Codex Desktop Computer Use 插件安装失败
    RepairComputerUse,
    /// 打开 API Key 设置窗口
    Settings,
    /// 设置 API Key（命令行，高级）
    Env {
        #[command(subcommand)]
        action: EnvAction,
    },
    /// 恢复 OpenAI 官方配置
    RestoreOpenai,
}

#[derive(Subcommand, Debug)]
pub enum EnvAction {
    /// 保存 API Key 到 ~/.codex-helper/.env
    Set {
        key: String,
        value: String,
    },
}
