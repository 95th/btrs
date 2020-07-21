use crate::contact::{CompactNodes, CompactNodesV6, ContactRef};
use crate::id::NodeId;
use crate::msg::recv::{ErrorResponse, Msg, Query, Response};
use crate::server::rpc::{RpcMgr, Transactions};
use crate::server::traversal::{BootstrapTraversal, GetPeersTraversal, Traversal};
use crate::table::RoutingTable;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;

mod rpc;
mod traversal;

type PeerSender = oneshot::Sender<Vec<SocketAddr>>;

pub struct Server {
    rpc: RpcMgr,
    table: RoutingTable,
    own_id: NodeId,
    next_refresh: Instant,
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
            next_refresh: Instant::now(),
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
        loop {
            if Instant::now() >= self.next_refresh {
                // refresh the table
                if self.table.is_empty() {
                    let target = self.own_id;
                    self.refresh(&target).await;
                } else if let Some(target) = self.table.pick_refresh_id() {
                    trace!("Bucket refresh target: {:?}", target);
                    self.submit_refresh(&target).await;
                }

                // Self refresh every 15 mins
                self.next_refresh = Instant::now() + Duration::from_secs(15 * 60);
            }

            // Check if any request from client such as Announce/Shutdown
            if self.check_client_request().await {
                debug!("Shutdown received from client");
                // TODO: Save DHT state on disk
                break;
            }

            // Wait for socket response
            self.recv_response(Duration::from_secs(1)).await;

            // Housekeep running requests
            self.check_running().await;
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

    async fn submit_refresh(&mut self, target: &NodeId) {
        let mut t = Box::new(BootstrapTraversal::new(target, &self.own_id));
        t.start(&mut self.table, &mut self.rpc).await;
        self.running.push(Traversal::Bootstrap(t));
    }

    async fn refresh(&mut self, target: &NodeId) {
        let mut t = Box::new(BootstrapTraversal::new(target, &self.own_id));
        t.start(&mut self.table, &mut self.rpc).await;

        loop {
            if t.invoke(&mut self.rpc).await {
                t.done();
                break;
            }

            t.prune(&mut self.table);

            let (msg, addr) = match self.rpc.recv_timeout(Duration::from_secs(1)).await {
                Ok(Some(x)) => x,
                Ok(None) => continue,
                Err(e) => {
                    warn!("{}", e);
                    continue;
                }
            };

            if let Msg::Response(resp) = msg {
                t.handle_reply(&resp, &addr, &mut self.table);
            }
        }

        debug!(
            "Table size:: live: {}, extra: {}",
            self.table.len(),
            self.table.len_extra()
        );
    }

    pub async fn get_peers(&mut self, info_hash: &NodeId, tx: PeerSender) {
        let mut t = Box::new(GetPeersTraversal::new(info_hash, &self.own_id, tx));
        t.start(&mut self.table, &mut self.rpc).await;
        self.running.push(Traversal::GetPeers(t));
    }

    async fn check_client_request(&mut self) -> bool {
        match self.client_rx.try_recv() {
            Ok(ClientRequest::GetPeers(info_hash, tx)) => {
                self.get_peers(&info_hash, tx).await;
                false
            }
            Ok(ClientRequest::Shutdown) => true,
            Err(_) => false,
        }
    }

    async fn recv_response(&mut self, timeout: Duration) {
        let (msg, addr) = match self.rpc.recv_timeout(timeout).await {
            Ok(Some(x)) => x,
            Ok(None) => return,
            Err(e) => {
                warn!("{}", e);
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
        debug!("Got query request: {:#?}", query);
    }

    fn handle_error(&mut self, err: &ErrorResponse) {
        debug!("Got query request: {:#?}", err);
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

        if let Some(token) = response.body.get_bytes("token") {
            self.tokens.insert(*response.id, token.to_vec());
        }

        Ok(())
    }
}
