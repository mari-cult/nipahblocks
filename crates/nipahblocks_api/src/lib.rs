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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum PlayerMessage {
    Message(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub user_id: u16,
    pub content: String,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ServerMessage {
    ChatMessage(ChatMessage),
    PlayerConnected(u16),
    PlayerDisconnected(u16),
}

impl TryFrom<ServerMessage> for tungstenite::Message {
    type Error = SerializeError;
    fn try_from(value: ServerMessage) -> Result<Self, Self::Error> {
        let data = bincode::serde::encode_to_vec(value, config::standard())?;
        Ok(tungstenite::Message::Binary(data.into()))
    }
}

impl TryFrom<PlayerMessage> for tungstenite::Message {
    type Error = SerializeError;
    fn try_from(value: PlayerMessage) -> Result<Self, Self::Error> {
        let data = bincode::serde::encode_to_vec(value, config::standard())?;
        Ok(tungstenite::Message::Binary(data.into()))
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
