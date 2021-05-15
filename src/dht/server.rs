use crate::dht::contact::{CompactNodes, CompactNodesV6, ContactRef};
use crate::dht::id::NodeId;
use crate::dht::msg::recv::{ErrorResponse, Msg, Query, Response};
use crate::dht::table::{Refresh, RoutingTable};
use rpc::{RpcMgr, Transactions};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;
use traversal::{
    AnnounceTraversal, BootstrapTraversal, GetPeersTraversal, PingTraversal, Traversal,
};

use super::future::poll_once;

mod rpc;
mod traversal;

type PeerSender = oneshot::Sender<HashSet<SocketAddr>>;

pub struct Server {
    rpc: RpcMgr,
    table: RoutingTable,
    own_id: NodeId,
    client_rx: Receiver<ClientRequest>,
    client_tx: Sender<ClientRequest>,
    running: Vec<Traversal>,
}

#[derive(Clone)]
pub struct Client {
    pub tx: Sender<ClientRequest>,
}

#[derive(Debug)]
pub enum ClientRequest {
    Announce(NodeId, PeerSender),
    GetPeers(NodeId, PeerSender),
    Shutdown,
}

impl Server {
    pub async fn new(port: u16, router_nodes: Vec<SocketAddr>) -> anyhow::Result<Server> {
        let addr = SocketAddr::from(([0u8; 4], port));
        let socket = UdpSocket::bind(addr).await?;
        let id = NodeId::gen();
        let (client_tx, client_rx) = mpsc::channel(100);

        let server = Server {
            rpc: RpcMgr::new(socket),
            table: RoutingTable::new(id, router_nodes),
            own_id: id,
            client_tx,
            client_rx,
            running: vec![],
        };

        Ok(server)
    }

    pub fn new_client(&self) -> Client {
        Client {
            tx: self.client_tx.clone(),
        }
    }

    pub async fn run(mut self) {
        log::debug!("Starting DHT server");
        let target = self.own_id;
        self.refresh(&target).await;

        loop {
            // refresh the table
            if let Some(refresh) = self.table.next_refresh() {
                match &refresh {
                    Refresh::Single(id, addr) => self.submit_ping(id, addr),
                    Refresh::Full(id) => self.submit_refresh(id),
                }
            }

            // Housekeep running requests
            self.check_running().await;

            // Check if any request from client such as Announce/Shutdown
            if self.check_client_request().await {
                log::debug!("Shutdown received from client");
                // TODO: Save DHT state on disk
                break;
            }

            // Wait for socket response
            self.recv_response(Duration::from_secs(1)).await;
        }
    }

    async fn check_running(&mut self) {
        let mut i = 0;
        while let Some(t) = self.running.get_mut(i) {
            t.prune(&mut self.table);

            if t.invoke(&mut self.rpc).await {
                let t = self.running.swap_remove(i);
                t.done();
            } else {
                i += 1;
            }
        }
    }

    fn submit_refresh(&mut self, target: &NodeId) {
        let traversal = Box::new(BootstrapTraversal::new(
            target,
            &self.own_id,
            &mut self.table,
        ));
        self.running.push(Traversal::Bootstrap(traversal));
    }

    fn submit_ping(&mut self, id: &NodeId, addr: &SocketAddr) {
        let t = Box::new(PingTraversal::new(&self.own_id, id, addr));
        self.running.push(Traversal::Ping(t));
    }

    async fn refresh(&mut self, target: &NodeId) {
        let mut traversal = Box::new(BootstrapTraversal::new(
            target,
            &self.own_id,
            &mut self.table,
        ));

        loop {
            if traversal.invoke(&mut self.rpc).await {
                traversal.done();
                break;
            }

            traversal.prune(&mut self.table);

            let (msg, addr) = match self.rpc.recv_timeout(Duration::from_secs(1)).await {
                Ok(Some(x)) => x,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("{}", e);
                    continue;
                }
            };

            if let Msg::Response(resp) = msg {
                traversal.handle_reply(&resp, &addr, &mut self.table);
            }
        }

        log::debug!(
            "Table size:: live: {}, extra: {}",
            self.table.len(),
            self.table.len_extra()
        );
    }

    pub fn submit_get_peers(&mut self, info_hash: &NodeId, tx: PeerSender) {
        let traversal = Box::new(GetPeersTraversal::new(
            info_hash,
            &self.own_id,
            tx,
            &mut self.table,
        ));
        self.running.push(Traversal::GetPeers(traversal));
    }

    pub fn submit_announce(&mut self, info_hash: &NodeId, tx: PeerSender) {
        let traversal = Box::new(AnnounceTraversal::new(
            info_hash,
            &self.own_id,
            tx,
            &mut self.table,
        ));
        self.running.push(Traversal::Announce(traversal));
    }

    /// Check for client requests.
    /// Returns `true` if server should shutdown.
    async fn check_client_request(&mut self) -> bool {
        let fut = poll_once(|cx| self.client_rx.poll_recv(cx));

        let req = match fut.await {
            // Got a request
            Some(Some(req)) => req,

            // Channel closed
            Some(None) => return true,

            // Not ready
            None => return false,
        };

        match req {
            ClientRequest::GetPeers(info_hash, tx) => {
                self.submit_get_peers(&info_hash, tx);
                false
            }
            ClientRequest::Announce(info_hash, tx) => {
                self.submit_announce(&info_hash, tx);
                false
            }
            ClientRequest::Shutdown => true,
        }
    }

    async fn recv_response(&mut self, timeout: Duration) {
        let (msg, addr) = match self.rpc.recv_timeout(timeout).await {
            Ok(Some(x)) => x,
            Ok(None) => return,
            Err(e) => {
                log::warn!("{}", e);
                return;
            }
        };

        match msg {
            Msg::Response(resp) => {
                for t in &mut self.running {
                    if t.handle_reply(&resp, &addr, &mut self.table) {
                        break;
                    }
                }
            }
            Msg::Query(query) => self.table.handle_query(&query),
            Msg::Error(err) => self.table.handle_error(&err),
        }
    }
}

impl RoutingTable {
    fn handle_query(&mut self, query: &Query) {
        log::debug!("Got query request: {:#?}", query);
    }

    fn handle_error(&mut self, err: &ErrorResponse) {
        log::debug!("Got query request: {:#?}", err);
    }

    fn read_nodes_with<F>(&mut self, response: &Response, mut f: F) -> anyhow::Result<()>
    where
        F: FnMut(&ContactRef),
    {
        if let Some(nodes) = response.body.get_bytes("nodes") {
            for c in CompactNodes::new(nodes)? {
                self.add_contact(&c);
                f(&c);
            }
        }

        if let Some(nodes6) = response.body.get_bytes("nodes6") {
            for c in CompactNodesV6::new(nodes6)? {
                self.add_contact(&c);
                f(&c);
            }
        }

        Ok(())
    }
}
