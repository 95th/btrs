use ben::{Decoder, Encoder, Parser};
use slab::Slab;
use tokio::net::UdpSocket;

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
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

use super::request::DhtTraversal;

pub struct RpcMgr<'a> {
    udp: &'a UdpSocket,
    txn_id: TxnId,
    pub own_id: NodeId,
    pub tokens: HashMap<SocketAddr, Vec<u8>>,
    pub txns: Transactions,
    pub buf: Vec<u8>,
}

impl<'a> RpcMgr<'a> {
    pub fn new(own_id: NodeId, udp: &'a UdpSocket) -> Self {
        Self {
            udp,
            txn_id: TxnId(0),
            own_id,
            tokens: HashMap::new(),
            txns: Transactions::new(),
            buf: Vec::with_capacity(1024),
        }
    }

    pub fn new_txn(&mut self) -> TxnId {
        self.txn_id.next_id()
    }

    pub async fn send(&mut self, addr: SocketAddr) -> anyhow::Result<()> {
        let to_write = self.buf.len();
        let result = self.udp.send_to(&self.buf, addr).await;
        self.buf.clear();
        let written = result?;
        anyhow::ensure!(written == to_write, "Couldn't write the whole message");
        Ok(())
    }

    pub async fn handle_response(
        &mut self,
        msg: Msg<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        running: &mut Slab<DhtTraversal>,
    ) {
        log::trace!("Received msg: {:?}", msg);

        match msg {
            Msg::Response(r) => self.handle_ok(r, addr, table, running).await,
            Msg::Error(e) => self.handle_error(e, addr, table, running).await,
            Msg::Query(q) => self.handle_query(q, addr, table).await,
        }
    }

    async fn handle_ok(
        &mut self,
        resp: Response<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        running: &mut Slab<DhtTraversal>,
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

            if let Some(t) = running.get_mut(req.traversal_id) {
                t.set_failed(&req.id, &addr);
                let done = t.add_requests(self).await;
                if done {
                    running.remove(req.traversal_id).done();
                }
            }
            return;
        }

        if let Some(t) = running.get_mut(req.traversal_id) {
            t.handle_response(&resp, &addr, table, self, req.has_id);
            let done = t.add_requests(self).await;
            if done {
                running.remove(req.traversal_id).done();
            }
        }
    }

    async fn handle_error(
        &mut self,
        err: ErrorResponse<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        running: &mut Slab<DhtTraversal>,
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

        if let Some(t) = running.get_mut(req.traversal_id) {
            t.set_failed(&req.id, &addr);
            let done = t.add_requests(self).await;
            if done {
                running.remove(req.traversal_id).done();
            }
        }
    }

    async fn handle_query(&mut self, query: Query<'_>, addr: SocketAddr, table: &mut RoutingTable) {
        table.heard_from(query.id);

        self.buf.clear();
        let mut dict = self.buf.add_dict();
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
            let d = p.parse::<Decoder>(&self.buf).unwrap();
            log::debug!("Sending reply: {:?}", d);
        }

        if let Err(e) = self.send(addr).await {
            log::warn!("Error in replying to query: {}", e);
        }
    }

    pub async fn check_timeouts(
        &mut self,
        table: &mut RoutingTable,
        running: &mut Slab<DhtTraversal>,
        timed_out: &mut Vec<(TxnId, Request)>,
    ) {
        let before = self.txns.pending.len();
        let cutoff = Instant::now() - self.txns.timeout;

        log::debug!(
            "{} pending txns in {} traversals",
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

            if let Some(t) = running.get_mut(req.traversal_id) {
                t.set_failed(&req.id, &req.addr);
                let done = t.add_requests(self).await;
                if done {
                    running.remove(req.traversal_id).done();
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
    pub traversal_id: usize,
}

impl Request {
    pub fn new(id: &NodeId, addr: &SocketAddr, traversal_id: usize) -> Self {
        Self {
            id: if id.is_zero() { NodeId::gen() } else { *id },
            addr: *addr,
            sent: Instant::now(),
            has_id: !id.is_zero(),
            traversal_id,
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

    pub fn insert(&mut self, txn_id: TxnId, id: &NodeId, addr: &SocketAddr, traversal_id: usize) {
        self.pending
            .insert(txn_id, Request::new(id, addr, traversal_id));
    }

    pub fn remove(&mut self, txn_id: TxnId) -> Option<Request> {
        self.pending.remove(&txn_id)
    }
}
