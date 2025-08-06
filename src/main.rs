use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
    routing::{get, get_service},
    Router,
};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use nix::pty::{openpty, Winsize};
use nix::unistd::{dup2, fork, setsid, ForkResult};
use std::os::unix::process::CommandExt;
use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process::{Command as StdCommand};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tower_http::services::ServeDir;
use uuid::Uuid;

type Sessions = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>;

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
struct AppState {
    sessions: Sessions,
    shell_command: String,
    shell_args: Vec<String>,
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
    
    let app_state = AppState {
        sessions: Arc::new(Mutex::new(HashMap::new())),
        shell_command: shell_command.clone(),
        shell_args: shell_args.clone(),
    };

    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .route("/api/shell-info", get(shell_info_handler))
        .nest_service("/", get_service(ServeDir::new("static")))
        .with_state(app_state);

    let bind_addr = format!("127.0.0.1:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap();
        
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
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

async fn shell_info_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let response = json!({
        "shell": state.shell_command,
        "args": state.shell_args,
        "full_command": format!("{} {}", state.shell_command, state.shell_args.join(" "))
    });
    
    axum::Json(response)
}

async fn websocket(socket: WebSocket, state: AppState) {
    let session_id = Uuid::new_v4().to_string();
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    state.sessions.lock().unwrap().insert(session_id.clone(), tx);

    // 创建 PTY
    let winsize = Winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    
    let pty_result = openpty(Some(&winsize), None).expect("Failed to open PTY");
    let master_fd = pty_result.master;
    let slave_fd = pty_result.slave;
    let master_raw_fd = master_fd.as_raw_fd();
    let slave_raw_fd = slave_fd.as_raw_fd();

    // Fork 进程
    let child_pid = match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => child,
        Ok(ForkResult::Child) => {
            // 子进程
            let _ = setsid();
            let _ = dup2(slave_raw_fd, 0); // stdin
            let _ = dup2(slave_raw_fd, 1); // stdout  
            let _ = dup2(slave_raw_fd, 2); // stderr
            
            // 设置环境变量
            unsafe {
                env::set_var("TERM", "xterm-256color");
                env::set_var("COLUMNS", "80");
                env::set_var("LINES", "24");
                env::set_var("FORCE_COLOR", "1");
                env::set_var("COLORTERM", "truecolor");
                
                if state.shell_command.contains("claude") {
                    env::set_var("PYTHONUNBUFFERED", "1");
                }
            }

            // 执行命令
            let mut cmd = StdCommand::new(&state.shell_command);
            for arg in &state.shell_args {
                cmd.arg(arg);
            }
            
            let _ = cmd.exec();
            std::process::exit(1);
        }
        Err(_) => panic!("Fork failed"),
    };

    // 父进程 - 转换为 tokio 异步 File
    let mut master_file = unsafe { tokio::fs::File::from_raw_fd(master_raw_fd) };
    
    // 读取 PTY 输出
    let sessions_for_read = state.sessions.clone();
    let session_id_for_read = session_id.clone();
    let mut master_read = master_file.try_clone().await.unwrap();
    tokio::spawn(async move {
        let mut buffer = [0u8; 1024];
        loop {
            match master_read.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => {
                    let output = String::from_utf8_lossy(&buffer[..n]).to_string();
                    if let Some(tx) = sessions_for_read.lock().unwrap().get(&session_id_for_read) {
                        let _ = tx.send(output);
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut send_task = tokio::spawn(async move {
        while let Some(output) = rx.recv().await {
            if sender.send(Message::Text(output)).await.is_err() {
                break;
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            if let Ok(msg) = msg {
                if let Message::Text(text) = msg {
                    if master_file.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => {
            recv_task.abort();
        },
        _ = (&mut recv_task) => {
            send_task.abort();
        }
    }

    // 清理
    let _ = nix::sys::signal::kill(child_pid, nix::sys::signal::Signal::SIGTERM);
    state.sessions.lock().unwrap().remove(&session_id);
}
