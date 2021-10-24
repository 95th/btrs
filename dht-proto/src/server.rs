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
        mut f: F,
        now: Instant,
    ) -> anyhow::Result<()>
    where
        F: FnMut(&ContactRef),
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
