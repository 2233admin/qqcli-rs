use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::{self, time::timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() -> Result<()> {
    let url = "ws://127.0.0.1:4301";
    println!("Connecting to {}...", url);

    let result = timeout(Duration::from_secs(30), connect_async(url)).await;

    match result {
        Ok(Ok((ws_stream, response))) => {
            println!("Connected! Status: {:?}", response.status());
            let (mut write, mut read) = ws_stream.split();

            let req = r#"{"action":".get_login_info","params":{}}"#;
            write.send(Message::Text(req.into())).await?;
            println!("Sent: {}", req);

            if let Some(msg) = timeout(Duration::from_secs(5), read.next()).await? {
                match msg? {
                    Message::Text(t) => println!("Response: {}", t),
                    Message::Binary(b) => println!("Binary: {:02X?}", b),
                    _ => println!("Other message"),
                }
            }
        }
        Ok(Err(e)) => {
            println!("Connection error: {}", e);
        }
        Err(_) => {
            println!("Connection timeout (30s)");
        }
    }

    Ok(())
}
