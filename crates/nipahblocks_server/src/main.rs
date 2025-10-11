use anyhow::Result;
use chrono::Utc;
use futures_util::StreamExt;
use log::{error, info};
use nipahblocks_api::{
    ChatMessage, PlayerId, PlayerMessage, Position, ServerMessage,
    chunk::{Chunk, ChunkId},
};
use noise::Perlin;
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
const NOISE_SEED: u32 = 123456;

struct PlayerState {
    id: PlayerId,
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

    async fn send_history(&self, messages: VecDeque<ChatMessage>) {
        for msg in messages {
            self.send_server_message(ServerMessage::ChatMessage(msg))
                .await;
        }
    }

    async fn send_player_connected(&self, player_id: PlayerId) {
        self.send_server_message(ServerMessage::PlayerConnected(player_id))
            .await;
    }

    async fn send_player_disconnected(&self, player_id: PlayerId) {
        self.send_server_message(ServerMessage::PlayerDisconnected(player_id))
            .await;
    }
    async fn send_player_list(&self, players: Vec<PlayerId>) {
        self.send_server_message(ServerMessage::Players(players))
            .await;
    }

    async fn send_chat_message(&self, message: ChatMessage) {
        self.send_server_message(ServerMessage::ChatMessage(message))
            .await;
    }

    async fn send_position_update(&self, player_id: PlayerId, position: Position) {
        self.send_server_message(ServerMessage::PlayerMoved(player_id, position))
            .await;
    }

    async fn send_chunk(&self, chunk: Chunk) {
        self.send_server_message(ServerMessage::Chunk(chunk)).await;
    }
}

struct State {
    history: RwLock<VecDeque<ChatMessage>>,
    players: RwLock<HashMap<PlayerId, PlayerState>>,
    chunks: RwLock<HashMap<ChunkId, Chunk>>,
    noise: Perlin,
}

impl State {
    fn new() -> Self {
        Self {
            history: RwLock::new(VecDeque::with_capacity(HISTORY_SIZE)),
            players: RwLock::new(HashMap::new()),
            chunks: RwLock::new(HashMap::new()),
            noise: Perlin::new(NOISE_SEED),
        }
    }

    async fn send_history(&self, player_id: PlayerId) {
        let messages = self.history.read().await.clone();
        self.players.read().await[&player_id]
            .send_history(messages)
            .await;
    }

    async fn send_player_connected(&self, player_id: PlayerId) {
        for (_, player) in self.players.read().await.iter() {
            player.send_player_connected(player_id).await;
        }
    }

    async fn send_player_disconnected(&self, player_id: PlayerId) {
        for (_, player) in self.players.read().await.iter() {
            player.send_player_disconnected(player_id).await;
        }
    }

    async fn send_chat_message(&self, player_id: PlayerId, content: String) {
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

    async fn update_player_position(&self, player_id: PlayerId, position: Position) {
        self.players
            .write()
            .await
            .entry(player_id)
            .and_modify(|player| player.position = position);
        for player in self.players.read().await.values() {
            player.send_position_update(player_id, position).await;
        }
    }

    async fn send_player_list(&self, player_id: PlayerId) {
        let players = self.players.read().await;
        let player_list = players.values().map(|player| player.id).collect();
        players[&player_id].send_player_list(player_list).await;
    }

    async fn send_chunk(&self, player_id: PlayerId, chunk_id: ChunkId) {
        let chunk = self
            .chunks
            .write()
            .await
            .entry(chunk_id)
            .or_insert(Chunk::new(&self.noise, chunk_id))
            .clone();
        self.players.read().await[&player_id]
            .send_chunk(chunk)
            .await;
    }

    async fn handle_player_message(&self, msg: PlayerMessage, player_id: PlayerId) {
        match msg {
            PlayerMessage::Message(content) => {
                self.send_chat_message(player_id, content).await;
            }
            PlayerMessage::UpdatePosition(pos) => {
                self.update_player_position(player_id, pos).await;
            }
            PlayerMessage::FetchPlayers => {
                self.send_player_list(player_id).await;
            }
            PlayerMessage::FetchChunk(chunk_id) => {
                self.send_chunk(player_id, chunk_id).await;
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
            let state = state.clone();
            async move {
                let msg: PlayerMessage = msg.try_into().unwrap();
                state.handle_player_message(msg, user_id).await
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
