use utp::{UtpListener, UtpSocket};

async fn handle_client(mut s: UtpSocket) {
    let mut buf = [0; 1500];

    // Reply to a data packet with its own payload, then end the connection
    match s.recv_from(&mut buf).await {
        Ok((nread, src)) => {
            println!("<= [{}] {:?}", src, &buf[..nread]);
            let _ = s.send_to(&buf[..nread]);
        }
        Err(e) => println!("{}", e),
    }
}

#[tokio::main]
async fn main() {
    // Start logger
    env_logger::init();

    // Create a listener
    let addr = "127.0.0.1:8080";
    let mut listener = UtpListener::bind(addr)
        .await
        .expect("Error binding listener");

    let mut incoming = listener.incoming();
    while let Some(connection) = incoming.next().await {
        // Spawn a new handler for each new connection
        match connection {
            Ok((socket, _src)) => {
                tokio::spawn(handle_client(socket));
            }
            _ => (),
        }
    }
}
