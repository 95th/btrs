use ben::{Decoder, Encoder, Parser};
use slab::Slab;

use crate::{
    bucket::Bucket,
    id::NodeId,
    msg::{
        recv::{ErrorResponse, Msg, Query, QueryKind, Response},
        TxnId,
    },
    table::RoutingTable,
};
use hashbrown::HashMap;
use std::{
    collections::{HashSet, VecDeque},
    fmt,
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

use super::{task::Task, TaskId};

pub struct RpcManager {
    txn_id: TxnId,
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
        running: &mut Slab<Box<dyn Task>>,
    ) {
        log::trace!("Received msg: {:?}", msg);

        match msg {
            Msg::Response(r) => self.handle_ok(r, addr, table, running),
            Msg::Error(e) => self.handle_error(e, addr, table, running),
            Msg::Query(q) => self.handle_query(q, addr, table),
        }
    }

    fn handle_ok(
        &mut self,
        resp: Response<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        running: &mut Slab<Box<dyn Task>>,
    ) {
        let req = match self.txns.remove(resp.txn_id) {
            Some(req) => req,
            None => {
                log::warn!("Response for unrecognized txn: {:?}", resp.txn_id);
                return;
            }
        };

        if req.has_id && &req.id == resp.id {
            table.heard_from(&req.id);
        } else if req.has_id {
            log::warn!(
                "ID mismatch from {}, Expected: {:?}, Actual: {:?}",
                addr,
                &req.id,
                &resp.id
            );
            table.failed(&req.id);

            if let Some(t) = running.get_mut(req.task_id) {
                t.set_failed(&req.id, &addr);
                let done = t.add_requests(self);
                if done {
                    running.remove(req.task_id).done(self);
                }
            }
            return;
        }

        if let Some(t) = running.get_mut(req.task_id) {
            t.handle_response(&resp, &addr, table, self, req.has_id);
            let done = t.add_requests(self);
            if done {
                running.remove(req.task_id).done(self);
            }
        }
    }

    fn handle_error(
        &mut self,
        err: ErrorResponse<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        running: &mut Slab<Box<dyn Task>>,
    ) {
        let req = match self.txns.remove(err.txn_id) {
            Some(req) => req,
            None => {
                log::warn!("Response for unrecognized txn: {:?}", err.txn_id);
                return;
            }
        };

        if req.has_id {
            table.failed(&req.id);
        }

        if let Some(t) = running.get_mut(req.task_id) {
            t.set_failed(&req.id, &addr);
            let done = t.add_requests(self);
            if done {
                running.remove(req.task_id).done(self);
            }
        }
    }

    fn handle_query(&mut self, query: Query<'_>, addr: SocketAddr, table: &mut RoutingTable) {
        table.heard_from(query.id);

        let mut buf = Vec::new();
        let mut dict = buf.add_dict();
        match addr.ip() {
            IpAddr::V4(a) => dict.add("ip", &a.octets()),
            IpAddr::V6(a) => dict.add("ip", &a.octets()),
        }

        let mut r = dict.add_dict("r");
        r.add("id", &self.own_id);

        match query.kind {
            QueryKind::Ping => {
                // Nothing else to add
            }
            QueryKind::FindNode | QueryKind::GetPeers => {
                let info_hash = match query.args.get_bytes("info_hash") {
                    Some(ih) if ih.len() == 20 => unsafe { &*(ih.as_ptr() as *const NodeId) },
                    _ => {
                        log::warn!("Valid info_hash not found in GET_PEERS query");
                        return;
                    }
                };

                let mut out = Vec::with_capacity(8);
                table.find_closest(info_hash, &mut out, Bucket::MAX_LEN);

                let nodes = &mut Vec::with_capacity(256);
                for c in out {
                    c.write_compact(nodes);
                }
                r.add("nodes", &nodes[..]);
            }
            QueryKind::AnnouncePeer => {
                log::warn!("Announce peer is not implemented fully");
            }
        }

        r.add("p", addr.port() as i64);
        r.finish();

        dict.add("t", query.txn_id);
        dict.add("y", "r");
        dict.finish();

        if log::log_enabled!(log::Level::Debug) {
            let mut p = Parser::new();
            let d = p.parse::<Decoder>(&buf).unwrap();
            log::debug!("Sending reply: {:?}", d);
        }

        self.reply(buf, addr);
    }

    pub fn check_timeouts(
        &mut self,
        table: &mut RoutingTable,
        running: &mut Slab<Box<dyn Task>>,
        timed_out: &mut Vec<(TxnId, Request)>,
    ) {
        if self.txns.pending.is_empty() {
            return;
        }

        let before = self.txns.pending.len();
        let cutoff = Instant::now() - self.txns.timeout;

        log::debug!(
            "{} pending txns in {} tasks",
            self.txns.pending.len(),
            running.len()
        );

        if self.txns.pending.is_empty() {
            assert!(running.is_empty());
        }

        timed_out.extend(self.txns.pending.drain_filter(|_, req| req.sent < cutoff));

        for (txn_id, req) in timed_out.drain(..) {
            log::trace!("Txn {:?} expired", txn_id);
            if req.has_id {
                table.failed(&req.id);
            }

            if let Some(t) = running.get_mut(req.task_id) {
                t.set_failed(&req.id, &req.addr);
                let done = t.add_requests(self);
                if done {
                    running.remove(req.task_id).done(self);
                }
            }
        }

        log::trace!(
            "Check timed out txns, before: {}, after: {}",
            before,
            self.txns.pending.len()
        );
    }
}

pub struct Request {
    pub id: NodeId,
    pub addr: SocketAddr,
    pub sent: Instant,
    pub has_id: bool,
    pub task_id: TaskId,
}

impl Request {
    pub fn new(id: &NodeId, addr: &SocketAddr, task_id: TaskId) -> Self {
        Self {
            id: if id.is_zero() { NodeId::gen() } else { *id },
            addr: *addr,
            sent: Instant::now(),
            has_id: !id.is_zero(),
            task_id,
        }
    }
}

pub struct Transactions {
    pending: HashMap<TxnId, Request>,
    timeout: Duration,
}

impl Transactions {
    pub fn new() -> Self {
        Self::with_timeout(Duration::from_secs(5))
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            timeout,
        }
    }

    pub fn insert(&mut self, txn_id: TxnId, id: &NodeId, addr: &SocketAddr, task_id: TaskId) {
        self.pending.insert(txn_id, Request::new(id, addr, task_id));
    }

    pub fn remove(&mut self, txn_id: TxnId) -> Option<Request> {
        self.pending.remove(&txn_id)
    }
}

pub enum Event {
    FoundPeers {
        task_id: TaskId,
        peers: HashSet<SocketAddr>,
    },
    Bootstrapped {
        task_id: TaskId,
    },
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

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FoundPeers { .. } => f.debug_struct("FoundPeers").finish(),
            Self::Bootstrapped { .. } => f.debug_struct("Bootstrapped").finish(),
            Self::Transmit { .. } => f.debug_struct("Transmit").finish(),
            Self::Reply { .. } => f.debug_struct("Reply").finish(),
        }
    }
}
