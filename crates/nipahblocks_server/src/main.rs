use anyhow::Result;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use nipahblocks_api::{ChatMessage, PlayerMessage, ServerMessage};
use std::{collections::VecDeque, env, fmt::Display, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, RwLock, broadcast},
};
use tokio_stream::wrappers::BroadcastStream;
use tokio_tungstenite::tungstenite;

const HISTORY_SIZE: usize = 50;
const BROADCAST_CH_SIZE: usize = 100;

struct State {
    history: RwLock<VecDeque<ChatMessage>>,
}

impl State {
    fn new() -> Self {
        Self {
            history: RwLock::new(VecDeque::with_capacity(HISTORY_SIZE)),
        }
    }

    async fn send_history<S: SinkExt<tungstenite::Message> + Unpin>(&self, sink: &mut S) {
        for msg in self
            .history
            .read()
            .await
            .iter()
            .map(|msg| ServerMessage::ChatMessage(msg.clone()))
            .filter_map(|msg| msg.try_into().ok())
        {
            sink.send(msg).await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = env_logger::try_init();
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let listener = TcpListener::bind(&addr).await?;
    info!("Listening on: {addr}");
    let state = Arc::new(State::new());
    let (broadcast_tx, _) =
        broadcast::channel::<tokio_tungstenite::tungstenite::Message>(BROADCAST_CH_SIZE);
    while let Ok((stream, _)) = listener.accept().await {
        let state = state.clone();
        let broadcast_tx = broadcast_tx.clone();
        tokio::spawn(accept_connection(stream, state, broadcast_tx));
    }
    Ok(())
}

async fn accept_connection(
    stream: TcpStream,
    state: Arc<State>,
    broadcast_tx: broadcast::Sender<tungstenite::Message>,
) {
    let addr = stream
        .peer_addr()
        .expect("connected streams should have a peer address");
    let user_id = addr.port();
    info!("Peer address: {}", addr);
    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");
    info!("New WebSocket connection: {}", addr);
    let broadcast_rx = BroadcastStream::new(broadcast_tx.subscribe());
    broadcast_tx
        .send(ServerMessage::PlayerConnected(user_id).try_into().unwrap())
        .unwrap();
    let (mut write, read) = ws_stream.split();
    state.send_history(&mut write).await;
    let write_task = broadcast_rx
        .map(|msg| {
            Ok(msg.unwrap_or_else(|e| {
                panic!("Failed to read message from broadcast for user {user_id}: {e}")
            }))
        })
        .forward(write);
    let read_task = read
        .filter_map(|msg| async {
            msg.inspect_err(|e| error!("Failed to read message from user {user_id}: {e}"))
                .ok()
        })
        .for_each(|msg| {
            let state = state.clone();
            let broadcast_tx = broadcast_tx.clone();
            async move {
                let msg: PlayerMessage = msg.try_into().unwrap();
                match msg {
                    PlayerMessage::Message(content) => {
                        let msg = ChatMessage {
                            user_id,
                            content,
                            time: Utc::now(),
                        };
                        {
                            let mut history = state.history.write().await;
                            if history.len() == HISTORY_SIZE {
                                history.pop_front();
                            }
                            history.push_back(msg.clone());
                        }
                        let reply = ServerMessage::ChatMessage(msg).try_into().unwrap();
                        broadcast_tx.send(reply).unwrap_or_else(|e| {
                            panic!("Failed to send broadcast message from user {user_id}: {e}")
                        });
                    }
                    _ => (),
                }
            }
        });
    tokio::select! {
        _ = read_task => info!("Read task for user {user_id} finished."),
        _ = write_task => info!("Write task for user {user_id} finished."),
    }
    broadcast_tx
        .send(
            ServerMessage::PlayerDisconnected(user_id)
                .try_into()
                .unwrap(),
        )
        .unwrap();
    info!("User {user_id} disconnected.");
}
