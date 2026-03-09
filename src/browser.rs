//! Browser control via Chrome DevTools Protocol (CDP).
//!
//! Control a Chrome/Chromium browser via CDP WebSocket connection.
//! Launch Chrome with `--remote-debugging-port=9222` to enable CDP.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

/// Information about a browser page/tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    pub id: String,
    pub url: String,
    pub title: String,
}

/// Information about a DOM element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementInfo {
    pub node_id: i64,
    pub tag: String,
    pub text: Option<String>,
    pub attributes: HashMap<String, String>,
}

/// Result of a screenshot capture.
#[derive(Debug, Clone)]
pub struct ScreenshotResult {
    /// PNG image bytes.
    pub data: Vec<u8>,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
}

/// CDP WebSocket client for communicating with Chrome DevTools.
pub struct CdpClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: AtomicU64,
    /// Pending responses keyed by request ID.
    pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Value>>>>,
    /// Background task handle for receiving messages.
    _receiver_handle: tokio::task::JoinHandle<()>,
}

impl CdpClient {
    /// Connect to a CDP WebSocket endpoint.
    pub async fn connect(ws_url: &str) -> Result<Self> {
        let (ws, _) = connect_async(ws_url)
            .await
            .context("Failed to connect to CDP WebSocket")?;

        let (write, read) = ws.split();
        let pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let pending_clone = pending.clone();
        let write = Arc::new(Mutex::new(write));

        // Spawn background task to receive messages
        let receiver_handle = tokio::spawn(async move {
            let mut read = read;
            while let Some(msg) = read.next().await {
                if let Ok(Message::Text(text)) = msg {
                    if let Ok(json) = serde_json::from_str::<Value>(&text) {
                        if let Some(id) = json.get("id").and_then(|v| v.as_u64()) {
                            let mut pending = pending_clone.lock().await;
                            if let Some(tx) = pending.remove(&id) {
                                let _ = tx.send(json).await;
                            }
                        }
                        // Events (no id) are ignored for now
                    }
                }
            }
        });

        // Reconstruct the WebSocket from the split parts
        let _ws_write = Arc::try_unwrap(write)
            .map_err(|_| anyhow!("Failed to unwrap write half"))?
            .into_inner();

        // We need to create a new connection since we split the original
        let (ws, _) = connect_async(ws_url)
            .await
            .context("Failed to reconnect to CDP WebSocket")?;

        Ok(Self {
            ws,
            next_id: AtomicU64::new(1),
            pending,
            _receiver_handle: receiver_handle,
        })
    }

    /// Send a CDP command and wait for response.
    pub async fn send(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = json!({
            "id": id,
            "method": method,
            "params": params,
        });

        let (tx, mut rx) = mpsc::channel(1);
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        self.ws
            .send(Message::Text(request.to_string()))
            .await
            .context("Failed to send CDP message")?;

        // Wait for response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv())
            .await
            .context("CDP response timeout")?
            .ok_or_else(|| anyhow!("CDP response channel closed"))?;

        if let Some(error) = response.get("error") {
            return Err(anyhow!(
                "CDP error: {}",
                error.get("message").and_then(|v| v.as_str()).unwrap_or("unknown")
            ));
        }

        Ok(response.get("result").cloned().unwrap_or(json!({})))
    }

    /// Send a CDP command without waiting for a specific response.
    pub async fn send_no_wait(&mut self, method: &str, params: Value) -> Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = json!({
            "id": id,
            "method": method,
            "params": params,
        });

        self.ws
            .send(Message::Text(request.to_string()))
            .await
            .context("Failed to send CDP message")?;

        Ok(())
    }

    /// Close the WebSocket connection.
    pub async fn close(&mut self) -> Result<()> {
        self.ws.close(None).await.context("Failed to close CDP connection")?;
        Ok(())
    }
}

/// Browser controller for high-level browser automation.
pub struct BrowserController {
    /// Chrome DevTools debug URL (e.g., "http://localhost:9222").
    debug_url: String,
    /// CDP client connected to a specific page.
    client: Option<CdpClient>,
    /// Currently attached page ID.
    current_page_id: Option<String>,
    /// DOM document root node ID.
    document_root_id: Option<i64>,
}

impl BrowserController {
    /// Connect to Chrome DevTools at the given debug URL.
    ///
    /// # Arguments
    /// * `debug_url` - Chrome DevTools debug URL (default: "http://localhost:9222")
    ///
    /// # Example
    /// ```no_run
    /// use rustclaw::browser::BrowserController;
    ///
    /// async fn example() {
    ///     let browser = BrowserController::connect("http://localhost:9222").await.unwrap();
    /// }
    /// ```
    pub async fn connect(debug_url: &str) -> Result<Self> {
        // Verify we can reach the debug endpoint
        let client = reqwest::Client::new();
        let version_url = format!("{}/json/version", debug_url);
        client
            .get(&version_url)
            .send()
            .await
            .context("Cannot reach Chrome DevTools. Is Chrome running with --remote-debugging-port?")?;

        Ok(Self {
            debug_url: debug_url.to_string(),
            client: None,
            current_page_id: None,
            document_root_id: None,
        })
    }

    /// List all open pages/tabs.
    pub async fn list_pages(&mut self) -> Result<Vec<PageInfo>> {
        let client = reqwest::Client::new();
        let list_url = format!("{}/json/list", self.debug_url);

        let response: Vec<Value> = client
            .get(&list_url)
            .send()
            .await?
            .json()
            .await
            .context("Failed to parse page list")?;

        let pages = response
            .into_iter()
            .filter_map(|page| {
                let page_type = page.get("type")?.as_str()?;
                if page_type != "page" {
                    return None;
                }
                Some(PageInfo {
                    id: page.get("id")?.as_str()?.to_string(),
                    url: page.get("url")?.as_str()?.to_string(),
                    title: page.get("title")?.as_str().unwrap_or("").to_string(),
                })
            })
            .collect();

        Ok(pages)
    }

    /// Attach to a specific page by ID. If no ID provided, attach to the first page.
    async fn attach_to_page(&mut self, page_id: Option<&str>) -> Result<()> {
        let pages = self.list_pages().await?;
        if pages.is_empty() {
            return Err(anyhow!("No pages available"));
        }

        let target_page = match page_id {
            Some(id) => pages
                .iter()
                .find(|p| p.id == id)
                .ok_or_else(|| anyhow!("Page {} not found", id))?,
            None => &pages[0],
        };

        let ws_url = format!(
            "{}/devtools/page/{}",
            self.debug_url.replace("http://", "ws://").replace("https://", "wss://"),
            target_page.id
        );

        let client = CdpClient::connect(&ws_url).await?;
        self.client = Some(client);
        self.current_page_id = Some(target_page.id.clone());
        self.document_root_id = None;

        // Enable required domains
        if let Some(ref mut client) = self.client {
            client.send("Page.enable", json!({})).await?;
            client.send("DOM.enable", json!({})).await?;
            client.send("Runtime.enable", json!({})).await?;
        }

        Ok(())
    }

    /// Ensure we have an active client connection.
    async fn ensure_connected(&mut self) -> Result<&mut CdpClient> {
        if self.client.is_none() {
            self.attach_to_page(None).await?;
        }
        self.client
            .as_mut()
            .ok_or_else(|| anyhow!("Not connected to any page"))
    }

    /// Get the document root node ID.
    async fn get_document_root(&mut self) -> Result<i64> {
        if let Some(root_id) = self.document_root_id {
            return Ok(root_id);
        }

        let client = self.ensure_connected().await?;
        let result = client.send("DOM.getDocument", json!({})).await?;

        let root_id = result
            .get("root")
            .and_then(|r| r.get("nodeId"))
            .and_then(|n| n.as_i64())
            .ok_or_else(|| anyhow!("Failed to get document root"))?;

        self.document_root_id = Some(root_id);
        Ok(root_id)
    }

    /// Navigate to a URL.
    pub async fn navigate(&mut self, url: &str) -> Result<()> {
        let client = self.ensure_connected().await?;

        client
            .send("Page.navigate", json!({ "url": url }))
            .await?;

        // Wait for page load
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Reset document root since DOM changed
        self.document_root_id = None;

        Ok(())
    }

    /// Get the page HTML content.
    pub async fn get_page_content(&mut self) -> Result<String> {
        let root_id = self.get_document_root().await?;
        let client = self.ensure_connected().await?;

        let result = client
            .send(
                "DOM.getOuterHTML",
                json!({ "nodeId": root_id }),
            )
            .await?;

        result
            .get("outerHTML")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Failed to get page content"))
    }

    /// Evaluate JavaScript expression and return result.
    pub async fn evaluate_js(&mut self, expression: &str) -> Result<Value> {
        let client = self.ensure_connected().await?;

        let result = client
            .send(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                }),
            )
            .await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Err(anyhow!(
                "JavaScript error: {}",
                exception
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            ));
        }

        Ok(result
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(json!(null)))
    }

    /// Click on an element matching the CSS selector.
    pub async fn click(&mut self, selector: &str) -> Result<()> {
        // Get element coordinates via JavaScript
        let js = format!(
            r#"
            (function() {{
                const el = document.querySelector({});
                if (!el) return null;
                const rect = el.getBoundingClientRect();
                return {{
                    x: rect.x + rect.width / 2,
                    y: rect.y + rect.height / 2
                }};
            }})()
            "#,
            serde_json::to_string(selector)?
        );

        let coords = self.evaluate_js(&js).await?;

        if coords.is_null() {
            return Err(anyhow!("Element not found: {}", selector));
        }

        let x = coords
            .get("x")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow!("Invalid coordinates"))?;
        let y = coords
            .get("y")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow!("Invalid coordinates"))?;

        let client = self.ensure_connected().await?;

        // Mouse down
        client
            .send(
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mousePressed",
                    "x": x,
                    "y": y,
                    "button": "left",
                    "clickCount": 1,
                }),
            )
            .await?;

        // Mouse up
        client
            .send(
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mouseReleased",
                    "x": x,
                    "y": y,
                    "button": "left",
                    "clickCount": 1,
                }),
            )
            .await?;

        Ok(())
    }

    /// Type text into an element matching the CSS selector.
    pub async fn type_text(&mut self, selector: &str, text: &str) -> Result<()> {
        // First click to focus the element
        self.click(selector).await?;

        // Small delay for focus
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let client = self.ensure_connected().await?;

        // Type each character
        for c in text.chars() {
            client
                .send(
                    "Input.dispatchKeyEvent",
                    json!({
                        "type": "keyDown",
                        "text": c.to_string(),
                    }),
                )
                .await?;

            client
                .send(
                    "Input.dispatchKeyEvent",
                    json!({
                        "type": "keyUp",
                        "text": c.to_string(),
                    }),
                )
                .await?;
        }

        Ok(())
    }

    /// Capture a screenshot of the current page.
    pub async fn screenshot(&mut self) -> Result<ScreenshotResult> {
        let client = self.ensure_connected().await?;

        let result = client
            .send(
                "Page.captureScreenshot",
                json!({
                    "format": "png",
                }),
            )
            .await?;

        let data_base64 = result
            .get("data")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("No screenshot data"))?;

        let data = base64::engine::general_purpose::STANDARD
            .decode(data_base64)
            .context("Failed to decode screenshot")?;

        // Get viewport dimensions
        let metrics = client
            .send("Page.getLayoutMetrics", json!({}))
            .await?;

        let width = metrics
            .get("cssVisualViewport")
            .and_then(|v| v.get("clientWidth"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1920.0) as u32;

        let height = metrics
            .get("cssVisualViewport")
            .and_then(|v| v.get("clientHeight"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1080.0) as u32;

        Ok(ScreenshotResult { data, width, height })
    }

    /// Wait for an element matching the selector to appear.
    pub async fn wait_for_selector(&mut self, selector: &str, timeout_ms: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            let js = format!(
                "document.querySelector({}) !== null",
                serde_json::to_string(selector)?
            );

            let exists = self.evaluate_js(&js).await?;
            if exists.as_bool().unwrap_or(false) {
                return Ok(());
            }

            if start.elapsed() > timeout {
                return Err(anyhow!(
                    "Timeout waiting for selector: {} ({}ms)",
                    selector,
                    timeout_ms
                ));
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Query all elements matching the CSS selector.
    pub async fn query_selector_all(&mut self, selector: &str) -> Result<Vec<ElementInfo>> {
        let root_id = self.get_document_root().await?;
        
        // First, get all node IDs and their details
        let node_details: Vec<(i64, String, HashMap<String, String>)> = {
            let client = self.ensure_connected().await?;

            let result = client
                .send(
                    "DOM.querySelectorAll",
                    json!({
                        "nodeId": root_id,
                        "selector": selector,
                    }),
                )
                .await?;

            let node_ids = result
                .get("nodeIds")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow!("No nodeIds in response"))?;

            let mut details = Vec::new();

            for node_id_val in node_ids {
                let node_id = node_id_val.as_i64().unwrap_or(0);
                if node_id == 0 {
                    continue;
                }

                // Get node details
                let node_result = client
                    .send(
                        "DOM.describeNode",
                        json!({ "nodeId": node_id }),
                    )
                    .await;

                if let Ok(node) = node_result {
                    if let Some(node_info) = node.get("node") {
                        let tag = node_info
                            .get("nodeName")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();

                        // Parse attributes
                        let mut attributes = HashMap::new();
                        if let Some(attrs) = node_info.get("attributes").and_then(|v| v.as_array()) {
                            for chunk in attrs.chunks(2) {
                                if let (Some(key), Some(val)) = (
                                    chunk.get(0).and_then(|v| v.as_str()),
                                    chunk.get(1).and_then(|v| v.as_str()),
                                ) {
                                    attributes.insert(key.to_string(), val.to_string());
                                }
                            }
                        }

                        details.push((node_id, tag, attributes));
                    }
                }
            }
            
            details
        };

        // Now get text content for each element (separate borrow scope)
        let mut elements = Vec::new();
        for (idx, (node_id, tag, attributes)) in node_details.into_iter().enumerate() {
            let js = format!(
                r#"
                (function() {{
                    const nodes = document.querySelectorAll({});
                    const node = nodes[{}];
                    return node ? node.textContent.trim().substring(0, 200) : null;
                }})()
                "#,
                serde_json::to_string(selector)?,
                idx
            );

            let text = self.evaluate_js(&js).await.ok().and_then(|v| {
                v.as_str().filter(|s| !s.is_empty()).map(|s| s.to_string())
            });

            elements.push(ElementInfo {
                node_id,
                tag,
                text,
                attributes,
            });
        }

        Ok(elements)
    }

    /// Close the browser connection.
    pub async fn close(&mut self) -> Result<()> {
        if let Some(ref mut client) = self.client {
            client.close().await?;
        }
        self.client = None;
        self.current_page_id = None;
        self.document_root_id = None;
        Ok(())
    }

    /// Get the currently attached page ID.
    pub fn current_page(&self) -> Option<&str> {
        self.current_page_id.as_deref()
    }

    /// Switch to a different page by ID.
    pub async fn switch_to_page(&mut self, page_id: &str) -> Result<()> {
        // Close current connection
        if let Some(ref mut client) = self.client {
            client.close().await.ok();
        }
        self.client = None;
        self.current_page_id = None;
        self.document_root_id = None;

        // Attach to new page
        self.attach_to_page(Some(page_id)).await
    }

    /// Create a new tab and navigate to URL.
    pub async fn new_tab(&mut self, url: &str) -> Result<PageInfo> {
        let client = reqwest::Client::new();
        let new_url = format!("{}/json/new?{}", self.debug_url, url);

        let response: Value = client
            .get(&new_url)
            .send()
            .await?
            .json()
            .await
            .context("Failed to create new tab")?;

        let page = PageInfo {
            id: response
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            url: response
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            title: response
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        };

        // Attach to the new page
        self.attach_to_page(Some(&page.id)).await?;

        Ok(page)
    }

    /// Close a tab by page ID.
    pub async fn close_tab(&mut self, page_id: &str) -> Result<()> {
        let client = reqwest::Client::new();
        let close_url = format!("{}/json/close/{}", self.debug_url, page_id);

        client
            .get(&close_url)
            .send()
            .await
            .context("Failed to close tab")?;

        // If we closed the current page, reset state
        if self.current_page_id.as_deref() == Some(page_id) {
            self.client = None;
            self.current_page_id = None;
            self.document_root_id = None;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_info_serialize() {
        let page = PageInfo {
            id: "123".to_string(),
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
        };
        let json = serde_json::to_string(&page).unwrap();
        assert!(json.contains("example.com"));
    }

    #[test]
    fn test_element_info() {
        let mut attrs = HashMap::new();
        attrs.insert("class".to_string(), "btn".to_string());

        let element = ElementInfo {
            node_id: 42,
            tag: "button".to_string(),
            text: Some("Click me".to_string()),
            attributes: attrs,
        };

        assert_eq!(element.tag, "button");
        assert_eq!(element.attributes.get("class"), Some(&"btn".to_string()));
    }

    #[test]
    fn test_screenshot_result() {
        let result = ScreenshotResult {
            data: vec![0x89, 0x50, 0x4E, 0x47], // PNG magic bytes
            width: 1920,
            height: 1080,
        };
        assert_eq!(result.width, 1920);
        assert!(result.data.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
    }
}
