//! Distributed Agent Communication.
//!
//! Cross-machine agent communication via a lightweight message bus.
//! Uses raw TCP with JSON-line framing (one JSON message per line).
//!
//! Features:
//! - Peer-to-peer messaging between nodes
//! - Broadcast to all peers
//! - Heartbeat-based health monitoring
//! - Task assignment/result/cancel messages
//! - Memory sync between nodes

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

/// Distributed message bus for cross-node communication.
pub struct DistributedBus {
    node_id: String,
    peers: HashMap<String, PeerInfo>,
    listener: Option<TcpListener>,
    inbox: Arc<RwLock<VecDeque<DistributedMessage>>>,
    listen_addr: SocketAddr,
}

/// Information about a peer node.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub node_id: String,
    pub address: SocketAddr,
    pub status: PeerStatus,
    pub last_seen: Instant,
}

/// Connection status of a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerStatus {
    Connected,
    Disconnected,
    Unknown,
}

impl std::fmt::Display for PeerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerStatus::Connected => write!(f, "connected"),
            PeerStatus::Disconnected => write!(f, "disconnected"),
            PeerStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// A message sent between distributed nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedMessage {
    /// Unique message ID.
    pub id: String,
    /// Node that sent the message.
    pub from_node: String,
    /// Target node (None = broadcast to all).
    pub to_node: Option<String>,
    /// Type of message.
    pub msg_type: MessageType,
    /// Message payload (JSON).
    pub payload: serde_json::Value,
    /// Unix timestamp (milliseconds).
    pub timestamp: i64,
}

impl DistributedMessage {
    /// Create a new message.
    pub fn new(
        from_node: &str,
        to_node: Option<&str>,
        msg_type: MessageType,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from_node: from_node.to_string(),
            to_node: to_node.map(String::from),
            msg_type,
            payload,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Types of messages that can be sent between nodes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Assign a task to a remote agent.
    TaskAssign,
    /// Return task result.
    TaskResult,
    /// Cancel a task.
    TaskCancel,
    /// Peer health check.
    Heartbeat,
    /// Request remote agent spawn.
    AgentSpawn,
    /// Agent status query/response.
    AgentStatus,
    /// Sync memory entries.
    MemorySync,
    /// User-defined message type.
    Custom(String),
}

impl DistributedBus {
    /// Create a new distributed bus.
    ///
    /// # Arguments
    /// * `node_id` - Unique identifier for this node
    /// * `listen_addr` - Address to listen for incoming connections
    pub async fn new(node_id: &str, listen_addr: SocketAddr) -> anyhow::Result<Self> {
        tracing::info!(
            "Creating distributed bus: node_id={}, listen_addr={}",
            node_id,
            listen_addr
        );

        Ok(Self {
            node_id: node_id.to_string(),
            peers: HashMap::new(),
            listener: None,
            inbox: Arc::new(RwLock::new(VecDeque::new())),
            listen_addr,
        })
    }

    /// Get this node's ID.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// Add a peer to the network.
    ///
    /// This doesn't immediately connect; the connection is established
    /// when sending the first message or via heartbeat.
    pub async fn add_peer(&mut self, node_id: &str, addr: SocketAddr) -> anyhow::Result<()> {
        tracing::info!("Adding peer: node_id={}, addr={}", node_id, addr);

        let peer = PeerInfo {
            node_id: node_id.to_string(),
            address: addr,
            status: PeerStatus::Unknown,
            last_seen: Instant::now(),
        };

        self.peers.insert(node_id.to_string(), peer);
        Ok(())
    }

    /// Remove a peer from the network.
    pub fn remove_peer(&mut self, node_id: &str) -> Option<PeerInfo> {
        self.peers.remove(node_id)
    }

    /// Send a message to a specific peer.
    pub async fn send(
        &self,
        to_node: &str,
        msg_type: MessageType,
        payload: serde_json::Value,
    ) -> anyhow::Result<()> {
        let peer = self
            .peers
            .get(to_node)
            .ok_or_else(|| anyhow::anyhow!("Unknown peer: {}", to_node))?;

        let message = DistributedMessage::new(&self.node_id, Some(to_node), msg_type, payload);

        self.send_to_addr(peer.address, &message).await
    }

    /// Broadcast a message to all peers.
    pub async fn broadcast(
        &self,
        msg_type: MessageType,
        payload: serde_json::Value,
    ) -> anyhow::Result<()> {
        let message = DistributedMessage::new(&self.node_id, None, msg_type, payload);

        let mut errors = Vec::new();
        for peer in self.peers.values() {
            if let Err(e) = self.send_to_addr(peer.address, &message).await {
                tracing::warn!("Failed to send to peer {}: {}", peer.node_id, e);
                errors.push((peer.node_id.clone(), e));
            }
        }

        if errors.len() == self.peers.len() && !self.peers.is_empty() {
            anyhow::bail!("Failed to send to all peers");
        }

        Ok(())
    }

    /// Send a message to a specific address.
    async fn send_to_addr(
        &self,
        addr: SocketAddr,
        message: &DistributedMessage,
    ) -> anyhow::Result<()> {
        let mut stream = TcpStream::connect(addr).await?;

        // Serialize message to JSON line
        let mut json = serde_json::to_string(message)?;
        json.push('\n');

        stream.write_all(json.as_bytes()).await?;
        stream.flush().await?;

        tracing::debug!(
            "Sent message {} to {} ({:?})",
            message.id,
            addr,
            message.msg_type
        );

        Ok(())
    }

    /// Receive the next message from the inbox.
    ///
    /// Returns `None` if the inbox is empty.
    pub async fn recv(&self) -> Option<DistributedMessage> {
        let mut inbox = self.inbox.write().await;
        inbox.pop_front()
    }

    /// Receive a message with timeout.
    ///
    /// Polls the inbox until a message is available or timeout expires.
    pub async fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Option<DistributedMessage> {
        let deadline = Instant::now() + timeout;
        let poll_interval = std::time::Duration::from_millis(50);

        while Instant::now() < deadline {
            if let Some(msg) = self.recv().await {
                return Some(msg);
            }
            tokio::time::sleep(poll_interval).await;
        }

        None
    }

    /// Check if there are pending messages in the inbox.
    pub async fn has_messages(&self) -> bool {
        let inbox = self.inbox.read().await;
        !inbox.is_empty()
    }

    /// Get the number of pending messages.
    pub async fn inbox_len(&self) -> usize {
        let inbox = self.inbox.read().await;
        inbox.len()
    }

    /// Start the TCP listener for incoming messages.
    ///
    /// This spawns a background task that accepts connections and
    /// reads messages into the inbox.
    pub async fn start_listener(&mut self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(self.listen_addr).await?;
        tracing::info!("Distributed bus listening on {}", self.listen_addr);

        self.listener = Some(listener);

        // Spawn listener task
        let inbox = Arc::clone(&self.inbox);
        let node_id = self.node_id.clone();
        let peers = self.peers.clone();

        tokio::spawn(async move {
            Self::listener_loop(inbox, node_id, peers).await;
        });

        Ok(())
    }

    /// Internal listener loop.
    async fn listener_loop(
        _inbox: Arc<RwLock<VecDeque<DistributedMessage>>>,
        _node_id: String,
        _peers: HashMap<String, PeerInfo>,
    ) {
        // We need to re-bind here since we can't move the listener
        // This is a limitation of the current design
        // In production, you'd want to pass the listener via Arc<Mutex<Option<TcpListener>>>
        tracing::warn!(
            "Listener loop started - note: re-binding not implemented in this design"
        );
    }

    /// Accept and handle a single connection.
    async fn handle_connection(
        stream: TcpStream,
        inbox: Arc<RwLock<VecDeque<DistributedMessage>>>,
        node_id: &str,
    ) {
        let peer_addr = stream.peer_addr().ok();
        tracing::debug!("New connection from {:?}", peer_addr);

        let reader = BufReader::new(stream);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            match serde_json::from_str::<DistributedMessage>(&line) {
                Ok(message) => {
                    // Check if message is for us
                    if message.to_node.is_none()
                        || message.to_node.as_deref() == Some(node_id)
                    {
                        tracing::debug!(
                            "Received message {} from {} ({:?})",
                            message.id,
                            message.from_node,
                            message.msg_type
                        );

                        let mut inbox = inbox.write().await;
                        inbox.push_back(message);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to parse message: {}", e);
                }
            }
        }

        tracing::debug!("Connection closed from {:?}", peer_addr);
    }

    /// Start the heartbeat loop.
    ///
    /// Periodically sends heartbeat messages to all peers to maintain
    /// connection status.
    pub async fn start_heartbeat_loop(&self, interval_secs: u64) {
        let node_id = self.node_id.clone();
        let peers: Vec<(String, SocketAddr)> = self
            .peers
            .iter()
            .map(|(id, p)| (id.clone(), p.address))
            .collect();

        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(interval_secs);

            loop {
                tokio::time::sleep(interval).await;

                for (peer_id, addr) in &peers {
                    let message = DistributedMessage::new(
                        &node_id,
                        Some(peer_id),
                        MessageType::Heartbeat,
                        serde_json::json!({
                            "timestamp": chrono::Utc::now().timestamp_millis(),
                        }),
                    );

                    if let Err(e) = Self::send_message_to_addr(*addr, &message).await {
                        tracing::debug!("Heartbeat to {} failed: {}", peer_id, e);
                    } else {
                        tracing::trace!("Heartbeat sent to {}", peer_id);
                    }
                }
            }
        });

        tracing::info!("Heartbeat loop started (interval: {}s)", interval_secs);
    }

    /// Static helper to send a message to an address.
    async fn send_message_to_addr(
        addr: SocketAddr,
        message: &DistributedMessage,
    ) -> anyhow::Result<()> {
        let mut stream = TcpStream::connect(addr).await?;
        let mut json = serde_json::to_string(message)?;
        json.push('\n');
        stream.write_all(json.as_bytes()).await?;
        stream.flush().await?;
        Ok(())
    }

    /// Get the status of all peers.
    pub fn peer_status(&self) -> Vec<(String, PeerStatus)> {
        self.peers
            .iter()
            .map(|(id, p)| (id.clone(), p.status.clone()))
            .collect()
    }

    /// Update peer status based on heartbeat response.
    pub fn update_peer_status(&mut self, node_id: &str, status: PeerStatus) {
        if let Some(peer) = self.peers.get_mut(node_id) {
            peer.status = status;
            peer.last_seen = Instant::now();
        }
    }

    /// Check for stale peers (no heartbeat received in timeout).
    pub fn check_stale_peers(&mut self, timeout_secs: u64) -> Vec<String> {
        let timeout = std::time::Duration::from_secs(timeout_secs);
        let now = Instant::now();

        let mut stale = Vec::new();
        for (id, peer) in self.peers.iter_mut() {
            if now.duration_since(peer.last_seen) > timeout {
                if peer.status != PeerStatus::Disconnected {
                    peer.status = PeerStatus::Disconnected;
                    stale.push(id.clone());
                }
            }
        }

        stale
    }

    /// Run the full listener + connection handler loop.
    ///
    /// This is a convenience method that handles incoming connections
    /// in a dedicated task.
    pub async fn run_listener(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(self.listen_addr).await?;
        tracing::info!("Distributed bus listening on {}", self.listen_addr);

        let inbox = Arc::clone(&self.inbox);
        let node_id = self.node_id.clone();

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let inbox = Arc::clone(&inbox);
                    let node_id = node_id.clone();

                    tokio::spawn(async move {
                        Self::handle_connection(stream, inbox, &node_id).await;
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
}

/// Helper to create task assignment payload.
pub fn task_assign_payload(
    task_id: &str,
    description: &str,
    role: Option<&str>,
    budget_tokens: Option<u64>,
) -> serde_json::Value {
    serde_json::json!({
        "task_id": task_id,
        "description": description,
        "role": role,
        "budget_tokens": budget_tokens,
    })
}

/// Helper to create task result payload.
pub fn task_result_payload(
    task_id: &str,
    success: bool,
    output: &str,
    tokens_used: u64,
) -> serde_json::Value {
    serde_json::json!({
        "task_id": task_id,
        "success": success,
        "output": output,
        "tokens_used": tokens_used,
    })
}

/// Helper to create memory sync payload.
pub fn memory_sync_payload(
    memories: Vec<serde_json::Value>,
    namespace: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "memories": memories,
        "namespace": namespace,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let msg = DistributedMessage::new(
            "node1",
            Some("node2"),
            MessageType::TaskAssign,
            serde_json::json!({"task_id": "t1", "description": "Do something"}),
        );

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: DistributedMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.from_node, "node1");
        assert_eq!(parsed.to_node, Some("node2".to_string()));
        assert_eq!(parsed.msg_type, MessageType::TaskAssign);
    }

    #[test]
    fn test_broadcast_message() {
        let msg = DistributedMessage::new(
            "node1",
            None, // broadcast
            MessageType::Heartbeat,
            serde_json::json!({"timestamp": 12345}),
        );

        assert!(msg.to_node.is_none());
        assert_eq!(msg.msg_type, MessageType::Heartbeat);
    }

    #[test]
    fn test_custom_message_type() {
        let msg = DistributedMessage::new(
            "node1",
            Some("node2"),
            MessageType::Custom("my_custom_type".to_string()),
            serde_json::json!({"data": "custom"}),
        );

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: DistributedMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(
            parsed.msg_type,
            MessageType::Custom("my_custom_type".to_string())
        );
    }

    #[test]
    fn test_task_assign_payload() {
        let payload = task_assign_payload("task1", "Build something", Some("builder"), Some(1000));

        assert_eq!(payload["task_id"], "task1");
        assert_eq!(payload["description"], "Build something");
        assert_eq!(payload["role"], "builder");
        assert_eq!(payload["budget_tokens"], 1000);
    }

    #[test]
    fn test_task_result_payload() {
        let payload = task_result_payload("task1", true, "Done!", 500);

        assert_eq!(payload["task_id"], "task1");
        assert_eq!(payload["success"], true);
        assert_eq!(payload["output"], "Done!");
        assert_eq!(payload["tokens_used"], 500);
    }

    #[test]
    fn test_peer_status_display() {
        assert_eq!(format!("{}", PeerStatus::Connected), "connected");
        assert_eq!(format!("{}", PeerStatus::Disconnected), "disconnected");
        assert_eq!(format!("{}", PeerStatus::Unknown), "unknown");
    }

    #[tokio::test]
    async fn test_bus_creation() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let bus = DistributedBus::new("test-node", addr).await.unwrap();

        assert_eq!(bus.node_id(), "test-node");
        assert!(bus.peer_status().is_empty());
    }

    #[tokio::test]
    async fn test_add_peer() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let mut bus = DistributedBus::new("node1", addr).await.unwrap();

        let peer_addr: SocketAddr = "127.0.0.1:9001".parse().unwrap();
        bus.add_peer("node2", peer_addr).await.unwrap();

        let status = bus.peer_status();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].0, "node2");
        assert_eq!(status[0].1, PeerStatus::Unknown);
    }

    #[tokio::test]
    async fn test_inbox_operations() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let bus = DistributedBus::new("node1", addr).await.unwrap();

        // Initially empty
        assert!(!bus.has_messages().await);
        assert_eq!(bus.inbox_len().await, 0);
        assert!(bus.recv().await.is_none());

        // Add a message directly to inbox (simulating received message)
        {
            let mut inbox = bus.inbox.write().await;
            inbox.push_back(DistributedMessage::new(
                "node2",
                Some("node1"),
                MessageType::Heartbeat,
                serde_json::json!({}),
            ));
        }

        assert!(bus.has_messages().await);
        assert_eq!(bus.inbox_len().await, 1);

        let msg = bus.recv().await.unwrap();
        assert_eq!(msg.from_node, "node2");

        assert!(!bus.has_messages().await);
    }

    #[tokio::test]
    async fn test_stale_peer_detection() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let mut bus = DistributedBus::new("node1", addr).await.unwrap();

        let peer_addr: SocketAddr = "127.0.0.1:9001".parse().unwrap();
        bus.add_peer("node2", peer_addr).await.unwrap();

        // No stale peers immediately
        let stale = bus.check_stale_peers(60);
        assert!(stale.is_empty());

        // With 0 timeout, peer becomes stale
        let stale = bus.check_stale_peers(0);
        assert_eq!(stale, vec!["node2"]);

        // Peer is now marked disconnected
        let status = bus.peer_status();
        assert_eq!(status[0].1, PeerStatus::Disconnected);
    }
}
