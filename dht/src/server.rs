use proto::{Event, NodeId};

use futures::{select, FutureExt};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    time::{Duration, Instant},
};
use tokio::{
    net::UdpSocket,
    time::{sleep_until, Instant as TokioInstant},
};

pub struct Dht {
    dht: proto::Dht,
    socket: UdpSocket,
    recv_buf: Vec<u8>,
}

impl Dht {
    pub async fn new(port: u16, router_nodes: Vec<SocketAddr>) -> anyhow::Result<Self> {
        let id = NodeId::gen();
        let now = Instant::now();

        let mut dht = proto::Dht::new(id, router_nodes, now);
        let socket = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, port)).await?;

        dht.add_request(proto::ClientRequest::Bootstrap { target: id }, now);

        Ok(Self {
            dht,
            socket,
            recv_buf: vec![0; 2048],
        })
    }

    pub async fn get_peers(&mut self, info_hash: NodeId) -> anyhow::Result<HashSet<SocketAddr>> {
        let req = proto::ClientRequest::Announce { info_hash };
        self.wait_for_peers(req).await
    }

    pub async fn announce(&mut self, info_hash: NodeId) -> anyhow::Result<HashSet<SocketAddr>> {
        let req = proto::ClientRequest::GetPeers { info_hash };
        self.wait_for_peers(req).await
    }

    async fn wait_for_peers(
        &mut self,
        req: proto::ClientRequest,
    ) -> anyhow::Result<HashSet<SocketAddr>> {
        if self.dht.add_request(req, Instant::now()).is_none() {
            return Ok(HashSet::new());
        }

        let timer = sleep_until(self.next_timeout());
        tokio::pin!(timer);

        loop {
            select! {
                // Wait for timer
                _ = timer.as_mut().fuse() => self.dht.tick(Instant::now()),

                // Listen for response
                resp = self.socket.recv_from(&mut self.recv_buf).fuse() => {
                    match resp {
                        Ok((len, addr)) => self.dht.receive(&self.recv_buf[..len], unmap_ipv4(addr), Instant::now()),
                        Err(e) => {
                            log::warn!("Error: {}", e);
                            continue;
                        },
                    }
                },

                complete => break,
            }

            if let Some(peers) = self.process_events().await {
                return Ok(peers);
            }

            timer.as_mut().reset(self.next_timeout());
        }

        Ok(HashSet::new())
    }

    async fn process_events(&mut self) -> Option<HashSet<SocketAddr>> {
        while let Some(event) = self.dht.poll_event() {
            log::debug!("Received event: {}", event);
            match event {
                Event::FoundPeers { peers } => return Some(peers),
                Event::Bootstrapped { .. } => {}
                Event::Transmit {
                    task_id,
                    node_id,
                    data,
                    target,
                } => match self.socket.send_to(&data, target).await {
                    Ok(n) if n == data.len() => {}
                    _ => self.dht.set_failed(task_id, &node_id, &target),
                },
                Event::Reply { data, target } => {
                    self.socket.send_to(&data, target).await.ok();
                }
            }
        }

        None
    }

    fn next_timeout(&self) -> TokioInstant {
        // 10 secs
        const DEFAULT_TIMER: Duration = Duration::from_secs(10);

        match self.dht.poll_timeout() {
            Some(t) => t.into(),
            None => TokioInstant::now() + DEFAULT_TIMER,
        }
    }
}

fn unmap_ipv4(addr: SocketAddr) -> SocketAddr {
    if let IpAddr::V6(ip) = addr.ip() {
        if let Some(ip) = ip.to_ipv4() {
            return SocketAddr::new(IpAddr::V4(ip), addr.port());
        }
    }

    addr
}
