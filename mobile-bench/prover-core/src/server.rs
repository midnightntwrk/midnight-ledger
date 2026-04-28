#![cfg(all(feature = "proof-server-http", not(target_os = "android")))]

use std::sync::mpsc;
use std::time::Duration;

use actix_web::dev::ServerHandle;
use midnight_proof_server::server;
use midnight_proof_server::worker_pool::WorkerPool;

pub struct LocalServer {
    handle: ServerHandle,
    port: u16,
}

impl LocalServer {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub async fn stop(self) {
        self.handle.stop(false).await;
    }
}

pub async fn spawn_local_server() -> std::io::Result<LocalServer> {
    let (tx, rx) = mpsc::channel::<std::io::Result<(ServerHandle, u16)>>();
    std::thread::spawn(move || {
        actix_web::rt::System::new().block_on(async move {
            let pool = WorkerPool::new(2, 2, 600.0);
            match server(0, false, pool) {
                Ok((srv, port)) => {
                    let _ = tx.send(Ok((srv.handle(), port)));
                    let _ = srv.await;
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });
    });
    let (handle, port) = rx
        .recv()
        .map_err(|_| std::io::Error::other("server thread died before sending handle"))??;

    for _ in 0..50 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return Ok(LocalServer { handle, port });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(std::io::Error::other("server did not become reachable"))
}
