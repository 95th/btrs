use crate::{
    contact::{CompactNodes, CompactNodesV6, ContactRef},
    id::NodeId,
    msg::{
        recv::{Msg, Response},
        TxnId,
    },
    server::task::Task,
    table::RoutingTable,
};
use ben::Parser;
use rpc::RpcManager;
use slab::Slab;
use std::{net::SocketAddr, time::Instant};

use self::{
    rpc::Request,
    task::{AnnounceTask, BootstrapTask, GetPeersTask, PingTask},
};

pub use rpc::Event;
pub use task::TaskId;

mod rpc;
mod task;

pub enum ClientRequest {
    Announce { info_hash: NodeId },
    GetPeers { info_hash: NodeId },
    Ping { id: NodeId, addr: SocketAddr },
    Bootstrap { target: NodeId },
}

pub struct Dht {
    table: RoutingTable,
    tasks: Slab<Box<dyn Task>>,
    timed_out: Vec<(TxnId, Request)>,
    parser: Parser,
    rpc: RpcManager,
}

impl Dht {
    pub fn new(id: NodeId, router_nodes: Vec<SocketAddr>, now: Instant) -> Self {
        Self {
            table: RoutingTable::new(id, router_nodes, now),
            tasks: Slab::new(),
            timed_out: vec![],
            parser: Parser::new(),
            rpc: RpcManager::new(id),
        }
    }

    pub fn is_idle(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.rpc.events.pop_front()
    }

    pub fn poll_timeout(&self) -> Option<Instant> {
        let txn_timeout = self.rpc.next_timeout();
        let table_timeout = self.table.next_timeout();

        match (txn_timeout, table_timeout) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    pub fn tick(&mut self, now: Instant) {
        log::trace!("Server::tick");
        self.rpc
            .check_timeouts(&mut self.table, &mut self.tasks, &mut self.timed_out, now);

        if let Some(refresh) = self.table.next_refresh(now) {
            log::trace!("Time to refresh the routing table");
            self.add_request(refresh, now);
        }
    }

    pub fn add_request(&mut self, request: ClientRequest, now: Instant) -> Option<TaskId> {
        use ClientRequest::*;

        let entry = self.tasks.vacant_entry();
        let tid = TaskId(entry.key());
        let table = &mut self.table;
        let mut task: Box<dyn Task> = match request {
            GetPeers { info_hash } => Box::new(GetPeersTask::new(&info_hash, table, tid)),
            Bootstrap { target } => Box::new(BootstrapTask::new(&target, table, tid)),
            Announce { info_hash } => Box::new(AnnounceTask::new(&info_hash, table, tid)),
            Ping { id, addr } => Box::new(PingTask::new(&id, &addr, tid)),
        };

        let done = task.add_requests(&mut self.rpc, now);
        if done {
            None
        } else {
            entry.insert(task);
            Some(tid)
        }
    }

    pub fn set_failed(&mut self, task_id: TaskId, id: &NodeId, addr: &SocketAddr) {
        if let Some(t) = self.tasks.get_mut(task_id.0) {
            t.set_failed(id, addr);
        }
    }

    pub fn receive(&mut self, buf: &[u8], addr: SocketAddr, now: Instant) {
        log::debug!("Got {} bytes from {}", buf.len(), addr);

        let msg = match self.parser.parse::<Msg>(buf) {
            Ok(x) => x,
            Err(e) => {
                log::warn!("Error parsing message from {}: {}", addr, e);
                return;
            }
        };

        self.rpc
            .handle_response(msg, addr, &mut self.table, &mut self.tasks, now);
    }
}

impl RoutingTable {
    fn read_nodes_with<F>(
        &mut self,
        response: &Response,
        now: Instant,
        mut f: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(&ContactRef<'_>),
    {
        if let Some(nodes) = response.body.get_bytes("nodes") {
            for c in CompactNodes::new(nodes)? {
                self.add_contact(&c, now);
                f(&c);
            }
        }

        if let Some(nodes6) = response.body.get_bytes("nodes6") {
            for c in CompactNodesV6::new(nodes6)? {
                self.add_contact(&c, now);
                f(&c);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, time::Duration};

    use ben::{DictEncoder, Encode};

    use crate::msg::{
        recv::QueryKind,
        send::{FindNode, GetPeers},
    };

    use super::*;

    #[test]
    fn idle_by_default() {
        let now = Instant::now();
        let mut dht = Dht::new(NodeId::gen(), vec![], now);
        assert!(dht.is_idle());
        assert_eq!(None, dht.poll_event());
    }

    #[test]
    fn bootstrap_without_router_fails() {
        let now = Instant::now();
        let id = NodeId::gen();
        let mut dht = Dht::new(id, vec![], now);
        let task_id = dht.add_request(ClientRequest::Bootstrap { target: id }, now);
        assert_eq!(None, task_id);
    }

    #[test]
    fn bootstrap() {
        let now = Instant::now();
        let id = NodeId::gen();
        let router = SocketAddr::from(([0u8; 16], 0));

        let mut dht = Dht::new(id, vec![router], now);
        let txn_id = dht.rpc.txn_id;
        let task_id = dht
            .add_request(ClientRequest::Bootstrap { target: id }, now)
            .unwrap();

        let event = dht.poll_event().unwrap();

        let find_node = FindNode {
            txn_id,
            id: &id,
            target: &id,
        };

        assert_eq!(
            event,
            Event::Transmit {
                task_id,
                node_id: NodeId::new(),
                data: find_node.encode_to_vec(),
                target: router,
            }
        );

        let buf = &mut vec![];
        let mut dict = DictEncoder::new(buf);
        dict.insert("ip", [0u8; 16]);
        let mut r = dict.insert_dict("r");
        r.insert("id", &id);
        r.insert("nodes", "");
        r.insert("p", 0);
        r.finish();

        dict.insert("t", txn_id);
        dict.insert("y", "r");
        dict.finish();

        dht.receive(buf, router, now);

        assert_eq!(Event::Bootstrapped { task_id }, dht.poll_event().unwrap());
        assert!(dht.is_idle());
        assert_eq!(None, dht.poll_event());
    }

    #[test]
    fn bootstrap_timeout() {
        let mut now = Instant::now();
        let id = NodeId::gen();
        let router = SocketAddr::from(([0u8; 16], 0));

        let mut dht = Dht::new(id, vec![router], now);
        let task_id = dht
            .add_request(ClientRequest::Bootstrap { target: id }, now)
            .unwrap();

        // Discard the transmit event
        dht.poll_event().unwrap();

        // 100 secs elapsed
        now += Duration::from_secs(100);

        dht.tick(now);

        assert_eq!(Event::Bootstrapped { task_id }, dht.poll_event().unwrap());
        assert!(dht.is_idle());
        assert_eq!(None, dht.poll_event());
    }

    #[test]
    fn get_peers() {
        let now = Instant::now();
        let id = NodeId::gen();
        let info_hash = NodeId::gen();
        let router = SocketAddr::from(([0u8; 16], 0));

        let mut dht = Dht::new(id, vec![router], now);
        let txn_id = dht.rpc.txn_id;
        let task_id = dht
            .add_request(ClientRequest::GetPeers { info_hash }, now)
            .unwrap();

        let event = dht.poll_event().unwrap();

        let find_node = GetPeers {
            txn_id,
            id: &id,
            info_hash: &info_hash,
        };

        assert_eq!(
            event,
            Event::Transmit {
                task_id,
                node_id: NodeId::new(),
                data: find_node.encode_to_vec(),
                target: router,
            }
        );

        let buf = &mut vec![];
        let mut dict = DictEncoder::new(buf);
        dict.insert("ip", [0u8; 16]);
        let mut r = dict.insert_dict("r");
        r.insert("id", &id);
        r.insert("nodes", "");
        r.insert("p", 0);
        r.insert("token", "hello");

        let mut values = r.insert_list("values");
        values.push([1, 2, 1, 2, 0, 2]);
        values.finish();

        r.finish();

        dict.insert("t", txn_id);
        dict.insert("y", "r");
        dict.finish();

        dht.receive(buf, router, now);

        assert_eq!(dht.rpc.tokens.get(&router).unwrap(), b"hello");

        assert_eq!(
            Event::FoundPeers {
                task_id,
                peers: [SocketAddr::from(([1, 2, 1, 2], 2))].into_iter().collect()
            },
            dht.poll_event().unwrap()
        );
        assert!(dht.is_idle());
        assert_eq!(None, dht.poll_event());
    }

    #[test]
    fn get_peers_timeout() {
        let mut now = Instant::now();
        let id = NodeId::gen();
        let info_hash = NodeId::gen();
        let router = SocketAddr::from(([0u8; 16], 0));

        let mut dht = Dht::new(id, vec![router], now);
        let task_id = dht
            .add_request(ClientRequest::GetPeers { info_hash }, now)
            .unwrap();

        // Discard the Transmit event
        dht.poll_event().unwrap();

        // 100 secs elapsed
        now += Duration::from_secs(100);

        dht.tick(now);

        assert_eq!(
            Event::FoundPeers {
                task_id,
                peers: HashSet::new()
            },
            dht.poll_event().unwrap()
        );
        assert!(dht.is_idle());
        assert_eq!(None, dht.poll_event());
    }

    #[test]
    fn require_table_refresh() {
        let mut now = Instant::now();
        let id = NodeId::gen();
        let router = SocketAddr::from(([0u8; 16], 0));

        let mut dht = Dht::new(id, vec![router], now);
        let txn_id = dht.rpc.txn_id;

        // 20 mins elapsed
        now += Duration::from_secs(20 * 60);
        dht.tick(now);

        assert_eq!(dht.tasks.len(), 1);

        let event = dht.poll_event().unwrap();

        let data = match event {
            Event::Transmit {
                task_id,
                data,
                target,
                ..
            } => {
                assert_eq!(task_id, dht.tasks[0].id());
                assert_eq!(target, router);
                data
            }
            _ => panic!("Unexpected event: {:?}", event),
        };

        let mut parser = Parser::new();
        let msg = parser.parse::<Msg>(&data).unwrap();

        match msg {
            Msg::Query(query) => {
                assert_eq!(query.id, &id);
                assert_eq!(query.txn_id, txn_id);
                assert!(matches!(query.kind, QueryKind::FindNode { .. }));
            }
            _ => panic!("Unexpected msg: {:?}", msg),
        }
    }
}
