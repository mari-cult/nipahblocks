use anyhow::Result;
use chrono::Utc;
use futures_util::StreamExt;
use log::{error, info};
use nipahblocks_api::{ChatMessage, ChunkId, PlayerMessage, Position, ServerMessage};
use std::{
    collections::{HashMap, VecDeque},
    env,
    sync::Arc,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{
        RwLock,
        mpsc::{self, Sender},
    },
};
use tokio_stream::wrappers::ReceiverStream;

const HISTORY_SIZE: usize = 50;
const PLAYER_CH_SIZE: usize = 100;

struct PlayerState {
    id: u16,
    position: Position,
    tx: Sender<ServerMessage>,
}

impl PlayerState {
    async fn send_server_message(&self, msg: ServerMessage) {
        self.tx
            .send(msg)
            .await
            .inspect_err(|e| error!("Failed to send message to user [{}] channel: {e}", self.id))
            .unwrap();
    }

    async fn send_history(&self, messages: impl IntoIterator<Item = ChatMessage>) {
        for msg in messages {
            self.send_server_message(ServerMessage::ChatMessage(msg))
                .await;
        }
    }

    async fn send_player_connected(&self, player_id: u16) {
        self.send_server_message(ServerMessage::PlayerConnected(player_id))
            .await;
    }

    async fn send_player_disconnected(&self, player_id: u16) {
        self.send_server_message(ServerMessage::PlayerDisconnected(player_id))
            .await;
    }

    async fn send_chat_message(&self, message: ChatMessage) {
        self.send_server_message(ServerMessage::ChatMessage(message))
            .await;
    }
}

struct State {
    history: RwLock<VecDeque<ChatMessage>>,
    players: RwLock<HashMap<u16, PlayerState>>,
}

impl State {
    fn new() -> Self {
        Self {
            history: RwLock::new(VecDeque::with_capacity(HISTORY_SIZE)),
            players: RwLock::new(HashMap::new()),
        }
    }

    async fn send_history(&self, player_id: u16) {
        let history = self.history.read().await;
        // Can't just use regular iter because some Send nonsense
        let messages = (0..history.len()).map(|idx| history[idx].clone());
        self.players.read().await[&player_id]
            .send_history(messages)
            .await;
    }

    async fn send_player_connected(&self, player_id: u16) {
        for (_, player) in self.players.read().await.iter() {
            player.send_player_connected(player_id).await;
        }
    }

    async fn send_player_disconnected(&self, player_id: u16) {
        for (_, player) in self.players.read().await.iter() {
            player.send_player_disconnected(player_id).await;
        }
    }

    async fn send_chat_message(&self, content: String, player_id: u16) {
        let message = ChatMessage {
            user_id: player_id,
            content,
            time: Utc::now(),
        };
        {
            let mut history = self.history.write().await;
            if history.len() == HISTORY_SIZE {
                history.pop_front();
            }
            history.push_back(message.clone());
        }
        for player in self.players.read().await.values() {
            player.send_chat_message(message.clone()).await;
        }
    }

    async fn update_player_position(&self, pos: Position, player_id: u16) {
        unimplemented!()
    }

    async fn send_player_list(&self, player_id: u16) {
        unimplemented!()
    }

    async fn send_chunk(&self, chunk_id: ChunkId, player_id: u16) {
        unimplemented!()
    }

    async fn handle_player_message(
        &self,
        msg: PlayerMessage,
        tx: Sender<ServerMessage>,
        player_id: u16,
    ) {
        match msg {
            PlayerMessage::Message(content) => {
                self.send_chat_message(content, player_id).await;
            }
            PlayerMessage::UpdatePosition(pos) => {
                self.update_player_position(pos, player_id).await;
            }
            PlayerMessage::FetchPlayers => {
                self.send_player_list(player_id).await;
            }
            PlayerMessage::FetchChunk(chunk_id) => {
                self.send_chunk(chunk_id, player_id).await;
            }
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
    while let Ok((stream, _)) = listener.accept().await {
        let state = state.clone();
        tokio::spawn(accept_connection(stream, state));
    }
    Ok(())
}

async fn accept_connection(stream: TcpStream, state: Arc<State>) {
    let addr = stream
        .peer_addr()
        .expect("connected streams should have a peer address");
    let user_id = addr.port();
    info!("Peer address: {}", addr);
    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");
    info!("New WebSocket connection: {}", addr);
    let (player_tx, player_rx) = mpsc::channel::<ServerMessage>(PLAYER_CH_SIZE);
    let player_rx = ReceiverStream::new(player_rx);
    state.players.write().await.insert(
        user_id,
        PlayerState {
            id: user_id,
            position: Position::default(),
            tx: player_tx.clone(),
        },
    );
    state.send_player_connected(user_id).await;
    state.send_history(user_id).await;
    let (write, read) = ws_stream.split();
    let write_task = player_rx
        .map(|msg| msg.try_into().unwrap())
        .map(Ok)
        .forward(write);
    let read_task = read
        .filter_map(|msg| async {
            msg.inspect_err(|e| error!("Failed to read message from user [{user_id}]: {e}"))
                .ok()
        })
        .for_each(|msg| {
            let tx = player_tx.clone();
            let state = state.clone();
            async move {
                let msg: PlayerMessage = msg.try_into().unwrap();
                state.handle_player_message(msg, tx, user_id).await
            }
        });
    tokio::select! {
        _ = read_task => info!("Read task for user {user_id} finished."),
        _ = write_task => info!("Write task for user {user_id} finished."),
    }
    state.players.write().await.remove(&user_id);
    state.send_player_disconnected(user_id).await;
    info!("User {user_id} disconnected.");
}
