// TODO: remove this once the rest of TCP is implemented
#[allow(dead_code)]
mod buf;
pub mod socket;
pub mod tsm;

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::RwLock;
use std::{net::Ipv4Addr, sync::Arc};

use crate::protocol::tcp::tsm::{Closed, Socket, TcpState};
use crate::route::PacketDecision::Drop;
use crate::{net::Net, protocol::ProtocolHandler, route::Router};
use async_trait::async_trait;
use etherparse::{Ipv4HeaderSlice, TcpHeaderSlice};
use socket::{TcpConn, TcpListener};

#[derive(Debug)]
pub struct TcpConnError {}

#[derive(Debug)]
pub struct TcpListenError {}

#[derive(Debug)]
pub struct TcpAcceptError {}

#[derive(Debug)]
pub struct TcpSendError {}

#[derive(Debug)]
pub struct TcpReadError {}

/// A TCP stack.
#[derive(Default)]
pub struct Tcp {
    port_mappings: RwLock<HashMap<u16, u16>>,
    states: RwLock<HashMap<u16, Socket>>,
    router: Arc<Router>,
    local_window_size: usize,
    // a concurrent data structure holding Tcp stack states
}

impl Tcp {
    pub fn new(router: Arc<Router>, local_window_size: usize) -> Self {
        Tcp {
            port_mappings: RwLock::new(HashMap::new()),
            states: RwLock::new(HashMap::new()),
            router,
            local_window_size,
        }
    }
    /// Attempts to connect to a host, establishing the client side of a TCP connection.
    pub async fn connect(&self, dest_ip: Ipv4Addr, port: u16) -> Result<TcpConn, TcpConnError> {
        // TODO: create Tcp state machine. State machine should
        // 1. Send syn packet, transition to SYN_SENT.
        // 2. When TCP handler receives syn+ack packet, send a syn packet and
        //    transition to ESTABLISHED.
        //
        // Tcp state machine should provide some function that blocks until
        // state becomes ESTABLISHED.

        let mut blank_state = Socket::new(port, self.router.clone(), self.local_window_size);
        let mut states = self.states.write().unwrap();
        blank_state
            .connect(dest_ip, port)
            .await
            .expect("TODO: panic message");
        states.insert(port, blank_state);
        drop(states);
        let syn_ack = receiver.try_recv().unwrap();
        Ok(())
    }

    /// Starts listening for incoming connections at a port. Opens a listener socket.
    pub async fn listen(&self, port: u16) -> Result<TcpListener, TcpListenError> {
        // TODO: create Tcp machine that starts with LISTEN state. Open listen socket.

        todo!()
    }
}

pub struct TcpHandler {
    tcp: Arc<Tcp>,
}

impl TcpHandler {
    pub fn new(tcp: Arc<Tcp>) -> Self {
        Self { tcp }
    }
}

#[async_trait]
impl ProtocolHandler for TcpHandler {
    async fn handle_packet<'a>(
        &self,
        header: &Ipv4HeaderSlice<'a>,
        payload: &[u8],
        _router: &Router,
        _net: &Net,
    ) {
        // Step 1: validate checksum
        let h = TcpHeaderSlice::from_slice(payload).unwrap();
        let dst_port = h.destination_port();
        let checksum = h.checksum();
        if checksum != h.calc_checksum_ipv4(header, payload).unwrap() {
            eprintln!("TCP checksum failed");
        }
        // Step 2: find the corresponding Tcp state machine
        let mut conns = self.tcp.states.write().unwrap();
        let tsm = conns.get_mut(&dst_port).unwrap();
        let tcp_payload = &payload[h.slice().len()..];
        // Step 3: pass the packet to the state machine
        tsm.handle_packet(header, &h, tcp_payload).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use tokio::sync::Barrier;

    use crate::{
        node::{Node, NodeBuilder},
        protocol::Protocol,
        Args,
    };

    use super::TcpHandler;

    #[tokio::test]
    async fn hello_world() {
        // A minimal test case that establishes TCP connection and sends some bytes.

        let send_cfg = crate::fixture::netlinks::abc::A.clone();
        let recv_cfg = crate::fixture::netlinks::abc::B.clone();
        let payload = "hello world!";
        let recv_listen_port = 5656;
        let barr = Arc::new(Barrier::new(2));

        let listen_barr = barr.clone();
        let sender_cfg = send_cfg.clone();
        let receiver_cfg = recv_cfg.clone();
        let sender = tokio::spawn(async move {
            let node = create_and_start_node(sender_cfg).await;
            listen_barr.wait().await;

            let dest_ip = {
                let recv_ips = receiver_cfg.get_my_interface_ips();
                recv_ips[0]
            };
            let conn = node.connect(dest_ip, recv_listen_port).await.unwrap();
            conn.send_all(payload.to_string().as_bytes()).await.unwrap();
        });

        let receiver = tokio::spawn(async move {
            let node = create_and_start_node(recv_cfg).await;

            let listener = node.listen(recv_listen_port).await.unwrap();
            let conn = listener.accept().await.unwrap();

            barr.wait().await;

            let mut buf = [0; 12];
            conn.read_all(&mut buf).await.unwrap();
            assert_eq!(String::from_utf8(buf.into()).unwrap(), payload.to_string());
        });

        sender.await.unwrap();
        receiver.await.unwrap();
    }

    async fn create_and_start_node(cfg: Args) -> Arc<Node> {
        let tcp_stack = Arc::new(Tcp::default());
        let node = Arc::new(
            NodeBuilder::new(&cfg, tcp_stack.clone())
                .with_protocol_handler(Protocol::Tcp, TcpHandler::new(tcp_stack))
                .build()
                .await,
        );
        let node_runner = node.clone();
        tokio::spawn(async move {
            node_runner.run().await;
        });
        node
    }
}
