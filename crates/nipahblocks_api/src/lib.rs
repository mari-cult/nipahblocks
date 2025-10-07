use bincode::{
    self,
    config::{self},
    error::{DecodeError, EncodeError},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SerializeError {
    #[error("Encoding error: {0}")]
    EncodeError(#[from] EncodeError),
}

#[derive(Error, Debug)]
pub enum DeserializeError {
    #[error("Decoding error: {0}")]
    DecodeError(#[from] DecodeError),
    #[error("Message isn't binary")]
    NotBinary,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Position {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ChunkId {
    x: i16,
    y: i16,
}

impl From<Position> for ChunkId {
    fn from(value: Position) -> Self {
        ChunkId {
            x: value.x.round() as i16,
            y: value.y.round() as i16,
        }
    }
}

impl From<ChunkId> for Position {
    fn from(value: ChunkId) -> Self {
        Position {
            x: value.x as f32,
            y: value.y as f32,
            z: 0.0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum PlayerMessage {
    Message(String),
    UpdatePosition(Position),
    FetchChunk(ChunkId),
    FetchPlayers,
}

pub type PlayerId = u16;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub user_id: PlayerId,
    pub content: String,
    pub time: DateTime<Utc>,
}

pub const CHUNK_WIDTH: usize = 16;
pub const CHUNK_HEIGHT: usize = 256;
pub const CHUNK_SIZE: usize = CHUNK_WIDTH * CHUNK_WIDTH * CHUNK_HEIGHT;

pub type BlockId = u8;

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Block(BlockId);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Chunk {
    id: ChunkId,
    blocks: Vec<Block>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ServerMessage {
    ChatMessage(ChatMessage),
    Players(Vec<PlayerId>),
    PlayerConnected(PlayerId),
    PlayerDisconnected(PlayerId),
    PlayerMoved(PlayerId, Position),
    Chunk(Chunk),
}

impl TryFrom<ServerMessage> for tungstenite::Message {
    type Error = SerializeError;
    fn try_from(value: ServerMessage) -> Result<Self, Self::Error> {
        bincode::serde::encode_to_vec(value, config::standard())
            .map(|data| tungstenite::Message::Binary(data.into()))
            .map_err(SerializeError::EncodeError)
    }
}

impl TryFrom<PlayerMessage> for tungstenite::Message {
    type Error = SerializeError;
    fn try_from(value: PlayerMessage) -> Result<Self, Self::Error> {
        bincode::serde::encode_to_vec(value, config::standard())
            .map(|data| tungstenite::Message::Binary(data.into()))
            .map_err(SerializeError::EncodeError)
    }
}

impl TryFrom<tungstenite::Message> for ServerMessage {
    type Error = DeserializeError;
    fn try_from(value: tungstenite::Message) -> Result<Self, Self::Error> {
        match value {
            tungstenite::Message::Binary(data) => {
                bincode::serde::decode_from_slice(&data, config::standard())
                    .map(|(x, _)| x)
                    .map_err(DeserializeError::DecodeError)
            }
            _ => Err(DeserializeError::NotBinary),
        }
    }
}

impl TryFrom<tungstenite::Message> for PlayerMessage {
    type Error = DeserializeError;
    fn try_from(value: tungstenite::Message) -> Result<Self, Self::Error> {
        match value {
            tungstenite::Message::Binary(data) => {
                bincode::serde::decode_from_slice(&data, config::standard())
                    .map(|(x, _)| x)
                    .map_err(DeserializeError::DecodeError)
            }
            _ => Err(DeserializeError::NotBinary),
        }
    }
}
