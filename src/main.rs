use futures_util::StreamExt;
use nipahblocks_api::{PlayerMessage, ServerMessage};
use std::env;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::mpsc::{self, Sender},
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[tokio::main]
async fn main() {
    let url = env::args()
        .nth(1)
        .unwrap_or_else(|| panic!("this program requires at least one argument"));
    let (stdin_tx, stdin_rx) = mpsc::channel(30);
    let rx_stream = ReceiverStream::new(stdin_rx);
    tokio::spawn(read_stdin(stdin_tx));
    let (ws_stream, _) = connect_async(&url).await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");
    let (write, read) = ws_stream.split();
    let stdin_to_ws = rx_stream.map(Ok).forward(write);
    let ws_to_stdout = {
        read.for_each(|message| async {
            let message: ServerMessage = message.unwrap().try_into().unwrap();
            match message {
                ServerMessage::PlayerConnected(user_id) => {
                    tokio::io::stdout()
                        .write_all(&format!("user {user_id} joined\n").into_bytes())
                        .await
                        .unwrap();
                }
                ServerMessage::PlayerDisconnected(user_id) => {
                    tokio::io::stdout()
                        .write_all(&format!("user {user_id} left\n").into_bytes())
                        .await
                        .unwrap();
                }
                ServerMessage::ChatMessage(msg) => {
                    tokio::io::stdout()
                        .write_all(
                            &format!(
                                "{} | [{}]: {}\n",
                                msg.time.format("%d,%H:%M"),
                                msg.user_id,
                                msg.content
                            )
                            .into_bytes(),
                        )
                        .await
                        .unwrap();
                }
                _ => (),
            }
        })
    };
    tokio::select! {
        _ = stdin_to_ws => (),
        _ = ws_to_stdout => (),
    }
}

// Our helper method which will read data from stdin and send it along the
// sender provided.
async fn read_stdin(tx: Sender<Message>) {
    let mut br = BufReader::new(tokio::io::stdin());
    let mut line = String::new();
    while let Ok(_) = br.read_line(&mut line).await {
        let msg = PlayerMessage::Message(line.trim().to_string())
            .try_into()
            .unwrap();
        tx.send(msg).await.expect("Failed to send stdin");
        line.clear();
    }
}
