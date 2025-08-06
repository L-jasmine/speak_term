use axum::{
    Router,
    extract::State,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::{get, get_service},
};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use pty_process::{Command as PtyCommand, Size, open};
use serde_json::json;
use std::env;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower_http::services::ServeDir;
use tokio::process::Child;
use pty_process::Pty;

#[derive(Parser)]
#[command(name = "web-terminal")]
#[command(about = "A web-based terminal application")]
struct Args {
    /// The shell/command to launch for new connections
    #[arg(short, long, default_value = "bash")]
    shell: String,

    /// Port to bind the server to
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Additional arguments to pass to the shell
    #[arg(long)]
    shell_args: Vec<String>,
}

#[derive(Clone)]
struct ShellConfig {
    command: String,
    args: Vec<String>,
}

enum TerminalEvent {
    PtyOutput(String),
    WebSocketInput(String),
    PtyEof,
    WebSocketClosed,
    ProcessExited,
    Error,
    Ignored, // 用于忽略的非文本消息
}

async fn wait_terminal_event(
    pty: &mut Pty,
    child: &mut Child,
    receiver: &mut futures_util::stream::SplitStream<WebSocket>,
    buffer: &mut [u8; 1024],
) -> TerminalEvent {
    tokio::select! {
        // 从 PTY 读取数据
        result = pty.read(buffer) => {
            match result {
                Ok(0) => TerminalEvent::PtyEof,
                Ok(n) => {
                    let output = String::from_utf8_lossy(&buffer[..n]).to_string();
                    TerminalEvent::PtyOutput(output)
                }
                Err(_) => TerminalEvent::Error,
            }
        },
        // 从 WebSocket 接收数据
        msg = receiver.next() => {
            match msg {
                Some(Ok(Message::Text(text))) => TerminalEvent::WebSocketInput(text),
                Some(Ok(_)) => {
                    // 忽略非文本消息，返回特殊事件让外层继续循环
                    TerminalEvent::Ignored
                },
                Some(Err(_)) | None => TerminalEvent::WebSocketClosed,
            }
        },
        // 等待子进程退出
        _ = child.wait() => TerminalEvent::ProcessExited,
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let shell_command = env::var("TERMINAL_SHELL").unwrap_or(args.shell);
    let mut shell_args = args.shell_args;

    // 对于常见的 shell 添加交互式参数
    if shell_command == "bash" && shell_args.is_empty() {
        shell_args.push("-i".to_string());
    } else if shell_command == "zsh" && shell_args.is_empty() {
        shell_args.push("-i".to_string());
    } else if shell_command == "fish" && shell_args.is_empty() {
        shell_args.push("-i".to_string());
    }

    let shell_config = ShellConfig {
        command: shell_command.clone(),
        args: shell_args.clone(),
    };

    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .route("/api/shell-info", get(shell_info_handler))
        .nest_service("/", get_service(ServeDir::new("static")))
        .with_state(shell_config);

    let bind_addr = format!("127.0.0.1:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();

    println!("Web terminal server running on http://{}", bind_addr);
    println!("Shell: {} {}", shell_command, shell_args.join(" "));
    println!("Press Ctrl+C to stop the server");

    // 处理 Ctrl+C 信号
    let server = axum::serve(listener, app);

    tokio::select! {
        _ = server => {},
        _ = tokio::signal::ctrl_c() => {
            println!("\nReceived Ctrl+C, shutting down...");
        }
    }
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(shell_config): State<ShellConfig>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, shell_config))
}

async fn shell_info_handler(State(shell_config): State<ShellConfig>) -> impl IntoResponse {
    let response = json!({
        "shell": shell_config.command,
        "args": shell_config.args,
        "full_command": format!("{} {}", shell_config.command, shell_config.args.join(" "))
    });

    axum::Json(response)
}

async fn websocket(socket: WebSocket, shell_config: ShellConfig) {
    let (mut sender, mut receiver) = socket.split();

    // 使用 pty-process 创建 PTY 和启动进程
    let size = Size::new(24, 80);
    let (pty, pts) = match open() {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Failed to create PTY: {}", e);
            return;
        }
    };

    pty.resize(size).expect("Failed to resize PTY");

    let mut cmd = PtyCommand::new(&shell_config.command);
    for arg in &shell_config.args {
        cmd = cmd.arg(arg);
    }

    // 设置环境变量
    cmd = cmd
        .env("TERM", "xterm-256color")
        .env("COLUMNS", "80")
        .env("LINES", "24")
        .env("FORCE_COLOR", "1")
        .env("COLORTERM", "truecolor");

    if shell_config.command.contains("claude") {
        cmd = cmd.env("PYTHONUNBUFFERED", "1");
    }

    let mut child = match cmd.spawn(pts) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Failed to spawn process: {}", e);
            return;
        }
    };

    let mut pty = pty;
    let mut buffer = [0u8; 1024];

    loop {
        let event = wait_terminal_event(&mut pty, &mut child, &mut receiver, &mut buffer).await;
        
        match event {
            TerminalEvent::PtyOutput(output) => {
                if sender.send(Message::Text(output)).await.is_err() {
                    break;
                }
            },
            TerminalEvent::WebSocketInput(text) => {
                if pty.write_all(text.as_bytes()).await.is_err() {
                    break;
                }
            },
            TerminalEvent::Ignored => {
                // 忽略的消息，继续下一次循环
                continue;
            },
            TerminalEvent::PtyEof | TerminalEvent::WebSocketClosed | 
            TerminalEvent::ProcessExited | TerminalEvent::Error => {
                break;
            },
        }
    }

    // 清理
    let _ = child.kill();
}
