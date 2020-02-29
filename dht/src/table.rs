use crate::entry::NodeEntry;

pub type Bucket = Vec<NodeEntry>;

pub struct RoutingNode {
    pub replacements: Bucket,
    pub live: Bucket,
}

pub struct RoutingTable {}
