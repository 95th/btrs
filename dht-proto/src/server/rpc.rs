use ben::{DictEncoder, Entry, Parser};
use slab::Slab;

use crate::{
    bucket::Bucket,
    id::NodeId,
    msg::{
        recv::{ErrorResponse, Msg, Query, QueryKind, Response},
        TxnId,
    },
    table::RoutingTable,
    util::WithBytes,
};
use hashbrown::HashMap;
use std::{
    collections::{HashSet, VecDeque},
    fmt,
    net::SocketAddr,
    time::{Duration, Instant},
};

use super::{task::Task, TaskId};

pub struct RpcManager {
    pub(crate) txn_id: TxnId,
    pub own_id: NodeId,
    pub tokens: HashMap<SocketAddr, Vec<u8>>,
    pub txns: Transactions,
    pub events: VecDeque<Event>,
}

impl RpcManager {
    pub fn new(own_id: NodeId) -> Self {
        Self {
            txn_id: TxnId(0),
            own_id,
            tokens: HashMap::new(),
            txns: Transactions::new(),
            events: VecDeque::new(),
        }
    }

    pub fn new_txn(&mut self) -> TxnId {
        self.txn_id.next_id()
    }

    pub fn transmit(&mut self, task_id: TaskId, node_id: NodeId, data: Vec<u8>, addr: SocketAddr) {
        self.add_event(Event::Transmit {
            task_id,
            node_id,
            data,
            target: addr,
        });
    }

    pub fn reply(&mut self, data: Vec<u8>, addr: SocketAddr) {
        self.add_event(Event::Reply { data, target: addr });
    }

    pub fn add_event(&mut self, event: Event) {
        self.events.push_back(event)
    }

    pub fn handle_response(
        &mut self,
        msg: Msg<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        tasks: &mut Slab<Box<dyn Task>>,
        now: Instant,
    ) {
        trace!("Received msg: {:?}", msg);

        match msg {
            Msg::Response(r) => self.handle_ok(r, addr, table, tasks, now),
            Msg::Error(e) => self.handle_error(e, addr, table, tasks, now),
            Msg::Query(q) => self.handle_query(q, addr, table, now),
        }
    }

    fn handle_ok(
        &mut self,
        resp: Response<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        tasks: &mut Slab<Box<dyn Task>>,
        now: Instant,
    ) {
        let req = match self.txns.remove(resp.txn_id) {
            Some(req) => req,
            None => {
                warn!("Response for unrecognized txn: {:?}", resp.txn_id);
                return;
            }
        };

        if req.has_id && req.id == resp.id {
            table.heard_from(req.id, now);
        } else if req.has_id {
            warn!(
                "ID mismatch, Expected: {:?}, Actual: {:?}",
                &req.id, &resp.id
            );
            table.failed(req.id);

            if let Some(task) = tasks.get_mut(req.task_id.0) {
                task.set_failed(req.id, addr);
                let done = task.add_requests(self, now);
                if done {
                    tasks.remove(req.task_id.0).done(self);
                }
            }
            return;
        }

        if let Some(task) = tasks.get_mut(req.task_id.0) {
            task.handle_response(&resp, addr, table, self, req.has_id, now);
            let done = task.add_requests(self, now);
            if done {
                tasks.remove(req.task_id.0).done(self);
            }
        }
    }

    fn handle_error(
        &mut self,
        err: ErrorResponse<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        tasks: &mut Slab<Box<dyn Task>>,
        now: Instant,
    ) {
        let req = match self.txns.remove(err.txn_id) {
            Some(req) => req,
            None => {
                warn!("Response for unrecognized txn: {:?}", err.txn_id);
                return;
            }
        };

        if req.has_id {
            table.failed(req.id);
        }

        if let Some(task) = tasks.get_mut(req.task_id.0) {
            task.set_failed(req.id, addr);
            let done = task.add_requests(self, now);
            if done {
                tasks.remove(req.task_id.0).done(self);
            }
        }
    }

    fn handle_query(
        &mut self,
        query: Query<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        now: Instant,
    ) {
        table.heard_from(query.id, now);

        let mut buf = Vec::new();
        let mut dict = DictEncoder::new(&mut buf);
        addr.ip().with_bytes(|b| dict.insert("ip", b));

        let mut r = dict.insert_dict("r");
        r.insert("id", &self.own_id);

        match query.kind {
            QueryKind::Ping => {
                // Nothing else to add
            }
            QueryKind::FindNode { target } | QueryKind::GetPeers { info_hash: target } => {
                let out = table.find_closest(target, Bucket::MAX_LEN);

                let nodes = &mut Vec::with_capacity(256);
                for c in out {
                    c.write_compact(nodes);
                }
                r.insert("nodes", &nodes[..]);
            }
            QueryKind::AnnouncePeer { .. } => {
                warn!("Announce peer query is not yet implemented");
            }
        }

        r.insert("p", addr.port() as i64);
        r.finish();

        dict.insert("t", query.txn_id);
        dict.insert("y", "r");
        dict.finish();

        debug!(
            "Sending reply: {:?}",
            Parser::new().parse::<Entry>(&buf).unwrap()
        );

        self.reply(buf, addr);
    }

    pub fn next_timeout(&self) -> Option<Instant> {
        self.txns.pending.values().map(|req| req.timeout).min()
    }

    pub fn check_timeouts(
        &mut self,
        table: &mut RoutingTable,
        tasks: &mut Slab<Box<dyn Task>>,
        now: Instant,
    ) {
        if self.txns.pending.is_empty() {
            return;
        }

        let before = self.txns.pending.len();

        debug!(
            "{} pending txns in {} tasks",
            self.txns.pending.len(),
            tasks.len()
        );

        if self.txns.pending.is_empty() {
            assert!(tasks.is_empty());
        }

        self.txns.collect_expired(now);

        while let Some((txn_id, req)) = self.txns.timed_out.pop() {
            trace!("Txn {:?} expired", txn_id);
            if req.has_id {
                table.failed(req.id);
            }

            if let Some(task) = tasks.get_mut(req.task_id.0) {
                task.set_failed(req.id, req.addr);
                let done = task.add_requests(self, now);
                if done {
                    tasks.remove(req.task_id.0).done(self);
                }
            }
        }

        trace!(
            "Check timed out txns, before: {}, after: {}",
            before,
            self.txns.pending.len()
        );
    }
}

pub struct Request {
    pub id: NodeId,
    pub addr: SocketAddr,
    pub timeout: Instant,
    pub has_id: bool,
    pub task_id: TaskId,
}

impl Request {
    pub fn new(id: NodeId, addr: SocketAddr, task_id: TaskId, timeout: Instant) -> Self {
        Self {
            id: if id.is_zero() { NodeId::gen() } else { id },
            addr,
            timeout,
            has_id: !id.is_zero(),
            task_id,
        }
    }
}

pub struct Transactions {
    pending: HashMap<TxnId, Request>,
    timed_out: Vec<(TxnId, Request)>,
    timeout: Duration,
}

impl Transactions {
    pub fn new() -> Self {
        Self::with_timeout(Duration::from_secs(5))
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            timed_out: Vec::new(),
            timeout,
        }
    }

    pub fn insert(
        &mut self,
        txn_id: TxnId,
        id: NodeId,
        addr: SocketAddr,
        task_id: TaskId,
        now: Instant,
    ) {
        self.pending
            .insert(txn_id, Request::new(id, addr, task_id, now + self.timeout));
    }

    pub fn remove(&mut self, txn_id: TxnId) -> Option<Request> {
        self.pending.remove(&txn_id)
    }

    pub fn collect_expired(&mut self, now: Instant) {
        self.timed_out
            .extend(self.pending.drain_filter(|_, req| req.timeout <= now));
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    FoundPeers {
        peers: HashSet<SocketAddr>,
    },
    Bootstrapped,
    Transmit {
        task_id: TaskId,
        node_id: NodeId,
        data: Vec<u8>,
        target: SocketAddr,
    },
    Reply {
        data: Vec<u8>,
        target: SocketAddr,
    },
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FoundPeers { .. } => f.debug_struct("FoundPeers").finish(),
            Self::Bootstrapped { .. } => f.debug_struct("Bootstrapped").finish(),
            Self::Transmit { task_id, .. } => f
                .debug_struct("Transmit")
                .field("task_id", task_id)
                .finish(),
            Self::Reply { .. } => f.debug_struct("Reply").finish(),
        }
    }
}
