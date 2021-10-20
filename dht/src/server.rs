use dht_proto::{ClientRequest as ProtoRequest, Dht as ProtoDht, Event, NodeId, TaskId};

use futures::{
    channel::{mpsc, oneshot},
    select, FutureExt, SinkExt, StreamExt,
};
use std::{
    collections::{HashMap, HashSet},
    net::{Ipv6Addr, SocketAddr},
    time::{Duration, Instant},
};
use tokio::{net::UdpSocket, time::interval};

pub type PeerSender = oneshot::Sender<HashSet<SocketAddr>>;

pub enum ClientRequest {
    Announce {
        info_hash: NodeId,
        sender: PeerSender,
    },
    GetPeers {
        info_hash: NodeId,
        sender: PeerSender,
    },
}

pub struct Dht {
    tx: mpsc::Sender<ClientRequest>,
}

impl Dht {
    pub fn new(port: u16, router_nodes: Vec<SocketAddr>) -> (Self, DhtDriver) {
        let (tx, rx) = mpsc::channel::<ClientRequest>(200);
        let id = NodeId::gen();

        let mut dht = ProtoDht::new(id, router_nodes, Instant::now());
        dht.add_request(ProtoRequest::Bootstrap { target: id });

        let driver = DhtDriver {
            port,
            rx: Some(rx),
            dht,
            pending: HashMap::new(),
        };

        (Self { tx }, driver)
    }

    pub async fn get_peers(&mut self, info_hash: NodeId) -> anyhow::Result<HashSet<SocketAddr>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(ClientRequest::GetPeers {
                info_hash,
                sender: tx,
            })
            .await?;

        Ok(rx.await?)
    }

    pub async fn announce(&mut self, info_hash: NodeId) -> anyhow::Result<HashSet<SocketAddr>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(ClientRequest::Announce {
                info_hash,
                sender: tx,
            })
            .await?;

        Ok(rx.await?)
    }
}

pub struct DhtDriver {
    port: u16,
    rx: Option<mpsc::Receiver<ClientRequest>>,
    dht: ProtoDht,
    pending: HashMap<TaskId, PeerSender>,
}

impl DhtDriver {
    pub async fn run(mut self) {
        let udp = match UdpSocket::bind((Ipv6Addr::UNSPECIFIED, self.port)).await {
            Ok(x) => x,
            Err(e) => {
                log::warn!("Cannot open UDP socket: {}", e);
                return;
            }
        };

        let recv_buf: &mut [u8] = &mut [0; 1024];
        let mut rx = self.rx.take().unwrap();

        let mut timer = interval(Duration::from_secs(1));

        // Wait for bootstrapping
        self.process_events(&udp).await;
        while !self.dht.is_idle() {
            select! {
                time = timer.tick().fuse() => {
                    self.dht.tick(time.into_std());
                    self.process_events(&udp).await;
                },

                // Listen for response
                resp = udp.recv_from(recv_buf).fuse() => {
                    let (n, addr) = match resp {
                        Ok(x) => x,
                        Err(e) => {
                            log::warn!("Error: {}", e);
                            continue;
                        },
                    };

                    self.dht.receive(&recv_buf[..n], addr);
                    self.process_events(&udp).await;
                }
            }
        }

        loop {
            select! {
                time = timer.tick().fuse() => {
                    self.dht.tick(time.into_std());
                    self.process_events(&udp).await;
                },

                // Listen for response
                resp = udp.recv_from(recv_buf).fuse() => {
                    let (n, addr) = match resp {
                        Ok(x) => x,
                        Err(e) => {
                            log::warn!("Error: {}", e);
                            continue;
                        },
                    };

                    self.dht.receive(&recv_buf[..n], addr);
                    self.process_events(&udp).await;
                },

                // Send requests
                request = rx.next() => {
                    let request = match request {
                        Some(x) => x,

                        // The channel is closed
                        None => break,
                    };

                    match request {
                        ClientRequest::Announce { info_hash, sender } => {
                            let req = ProtoRequest::Announce { info_hash };
                            if let Some(id) = self.dht.add_request(req) {
                                self.pending.insert(id, sender);
                            }
                        },
                        ClientRequest::GetPeers { info_hash, sender } => {
                            let req = ProtoRequest::GetPeers { info_hash };
                            if let Some(id) = self.dht.add_request(req) {
                                self.pending.insert(id, sender);
                            }
                        },
                    };
                    self.process_events(&udp).await;
                },
                complete => break,
            }
        }
    }

    async fn process_events(&mut self, socket: &UdpSocket) {
        while let Some(event) = self.dht.poll() {
            log::debug!("Received event: {:?}", event);
            match event {
                Event::FoundPeers { task_id: id, peers } => {
                    if let Some(sender) = self.pending.remove(&id) {
                        let _ = sender.send(peers);
                    }
                }
                Event::Bootstrapped { .. } => {}
                Event::Transmit {
                    task_id,
                    node_id,
                    data,
                    target,
                } => match socket.send_to(&data, target).await {
                    Ok(n) if n == data.len() => {}
                    _ => self.dht.set_failed(task_id, &node_id, &target),
                },
                Event::Reply { data, target } => {
                    socket.send_to(&data, target).await.ok();
                }
            }
        }
    }
}
