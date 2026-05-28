use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() -> Result<()> {
    println!("Connecting to ws://127.0.0.1:4301...");
    let (ws_stream, _) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        connect_async("ws://127.0.0.1:4301")
    ).await??;
    
    println!("Connected!");
    let (mut write, mut read) = ws_stream.split();
    
    // Send login info request
    let req = r#"{"action":".get_login_info","params":{}}"#;
    write.send(Message::Text(req.into())).await?;
    println!("Sent: {}", req);
    
    // Receive response
    if let Some(msg) = tokio::time::timeout(std::time::Duration::from_secs(5), read.next()).await? {
        match msg? {
            Message::Text(t) => println!("Received: {}", t),
            Message::Binary(b) => println!("Binary: {:02X?}", b),
            _ => println!("Other: {:?}", msg),
        }
    }
    
    Ok(())
}
