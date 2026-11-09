//! WebSocket link to the game server with automatic reconnect.

use crate::game::NetEvent;
use chroma_protocol::{ClientMsg, ServerMsg};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, MessageEvent, WebSocket};

const RECONNECT_MS: f64 = 2000.0;

type Inbox = Rc<RefCell<VecDeque<NetEvent>>>;

pub struct Net {
    url: String,
    ws: Option<WebSocket>,
    inbox: Inbox,
    retry_at: f64,
    // Kept alive as long as the socket they are attached to.
    _handlers: Vec<Box<dyn std::any::Any>>,
}

/// `ws://<host>:8090/ws` for local development, `wss://ws.<host>/ws` otherwise.
pub fn default_url() -> String {
    let host = web_sys::window().expect("no window").location().hostname().expect("location.hostname failed");
    if host == "localhost" || host == "127.0.0.1" {
        format!("ws://{host}:8090/ws")
    } else {
        format!("wss://ws.{host}/ws")
    }
}

impl Net {
    pub fn new(url: String) -> Self {
        let mut net = Net { url, ws: None, inbox: Rc::new(RefCell::new(VecDeque::new())), retry_at: 0.0, _handlers: Vec::new() };
        net.connect();
        net
    }

    fn connect(&mut self) {
        let ws = WebSocket::new(&self.url).unwrap_or_else(|e| panic!("WebSocket({}) failed: {e:?}", self.url));

        let inbox = self.inbox.clone();
        let on_open = Closure::<dyn FnMut()>::new(move || inbox.borrow_mut().push_back(NetEvent::Open));
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));

        let inbox = self.inbox.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
            let text = ev.data().as_string().expect("server sent a non-text frame");
            let msg: ServerMsg =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("bad server message {text}: {e}"));
            inbox.borrow_mut().push_back(NetEvent::Message(msg));
        });
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        // Errors are always followed by a close event, which carries the details we report.
        let inbox = self.inbox.clone();
        let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |ev: CloseEvent| {
            let reason = if ev.reason().is_empty() {
                format!("соединение закрыто (код {})", ev.code())
            } else {
                format!("соединение закрыто (код {}): {}", ev.code(), ev.reason())
            };
            inbox.borrow_mut().push_back(NetEvent::Closed(reason));
        });
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        if let Some(old) = self.ws.replace(ws) {
            old.set_onopen(None);
            old.set_onmessage(None);
            old.set_onclose(None);
        }
        self._handlers = vec![Box::new(on_open), Box::new(on_message), Box::new(on_close)];
    }

    /// Drains network events and reconnects after a close.
    pub fn poll(&mut self, now_ms: f64) -> Vec<NetEvent> {
        let events: Vec<NetEvent> = self.inbox.borrow_mut().drain(..).collect();
        if events.iter().any(|e| matches!(e, NetEvent::Closed(_))) {
            if let Some(old) = self.ws.take() {
                old.set_onopen(None);
                old.set_onmessage(None);
                old.set_onclose(None);
            }
            self.retry_at = now_ms + RECONNECT_MS;
        }
        if self.ws.is_none() && now_ms >= self.retry_at {
            self.connect();
        }
        events
    }

    pub fn is_open(&self) -> bool {
        self.ws.as_ref().is_some_and(|ws| ws.ready_state() == WebSocket::OPEN)
    }

    pub fn send(&self, msg: &ClientMsg) {
        let ws = self.ws.as_ref().expect("send on a missing socket");
        let text = serde_json::to_string(msg).expect("ClientMsg serialization failed");
        ws.send_with_str(&text).expect("WebSocket.send failed");
    }
}
