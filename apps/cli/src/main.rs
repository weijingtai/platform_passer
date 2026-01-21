use anyhow::Result;
use clap::{Parser, Subcommand};
use platform_passer_session::{run_client_session, run_server_session, SessionEvent};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{info, error};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start as the processing server (Input Sink)
    Server {
        #[arg(short, long, default_value = "0.0.0.0:4433")]
        bind: SocketAddr,
    },
    /// Start as the capturing client (Input Source)
    Client {
        #[arg(short, long, default_value = "127.0.0.1:4433")]
        server: SocketAddr,
        #[arg(long)]
        send_file: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Server { bind } => run_server(bind).await,
        Commands::Client { server, send_file } => run_client(server, send_file).await,
    }
}

async fn run_server(bind_addr: SocketAddr) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(100);
    
    // Spawn session
    tokio::spawn(async move {
        if let Err(e) = run_server_session(bind_addr, tx.clone()).await {
             let _ = tx.send(SessionEvent::Error(e.to_string())).await;
        }
    });

    // Handle events
    while let Some(event) = rx.recv().await {
        match event {
            SessionEvent::Log(msg) => info!("{}", msg),
            SessionEvent::Connected(addr) => info!("Connected: {}", addr),
            SessionEvent::Disconnected => info!("Disconnected"),
            SessionEvent::Error(msg) => error!("{}", msg),
        }
    }
    Ok(())
}

async fn run_client(server_addr: SocketAddr, send_file_path: Option<PathBuf>) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(100);
    // CLI doesn't use dynamic commands yet, so just pass a dummy receiver
    let (_cmd_tx, cmd_rx) = mpsc::channel(1); 

     tokio::spawn(async move {
        if let Err(e) = run_client_session(server_addr, send_file_path, cmd_rx, tx.clone()).await {
             let _ = tx.send(SessionEvent::Error(e.to_string())).await;
        }
    });

    while let Some(event) = rx.recv().await {
        match event {
            SessionEvent::Log(msg) => info!("{}", msg),
            SessionEvent::Connected(addr) => info!("Connected: {}", addr),
            SessionEvent::Disconnected => info!("Disconnected"),
            SessionEvent::Error(msg) => error!("{}", msg),
        }
    }
    Ok(())
}
