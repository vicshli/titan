mod ack_policy;
#[allow(dead_code)]
mod buf;
pub mod prelude;
mod socket;
mod transport;

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use std::usize;

use crate::drop_policy::DropPolicy;
use crate::net::Net;
use crate::protocol::tcp::socket::UpdateAction;
use crate::{net::vtlink::VtLinkNet, protocol::ProtocolHandler};
use async_trait::async_trait;
use etherparse::{Ipv4HeaderSlice, TcpHeaderSlice};
use socket::Socket;
pub use socket::{TcpConn, TcpListener};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{RwLock, RwLockReadGuard};

use self::prelude::{Port, Remote, SocketDescriptor, SocketId};
use self::socket::{SocketStatus, SynReceived, TransportError};

pub const TCP_DEFAULT_WINDOW_SZ: usize = (1 << 16) - 1;

/// The maximum payload size for each TCP packet.
pub const MAX_SEGMENT_SZ: usize = 1024;

// The maximum number of TCP connections that are waiting to be accepted on a
// listener port.
pub const MAX_PENDING_TCP_CONNECTIONS: usize = 1024;

pub const TCP_DEFAULT_CONNECTION_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Copy, Clone)]
pub enum TcpConnError {
    ConnectionExists(Remote),
    Transport(TransportError),
    Timeout,
}

#[derive(Debug)]
pub enum TcpListenError {
    PortOccupied(Port),
}

#[derive(Debug)]
pub enum TcpAcceptError {
    ListenSocketClosed,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TcpSendError {
    NoSocket(SocketDescriptor),
    ConnNotEstablished,
    ConnClosed,
}

#[derive(Debug)]
pub enum TcpReadError {
    NoSocket(SocketDescriptor),
    /// Failed to fill the provided buffer because the remote has closed.
    /// Returns the number of bytes written into the buffer.
    Closed(usize),
    ConnNotEstablished,
}

#[derive(Debug)]
pub enum TcpCloseError {
    NoSocketOnDescriptor(SocketDescriptor),
    NoSocketOnId(SocketId),
    AlreadyClosed,
}

/// A TCP stack.
pub struct Tcp<N: Net + 'static> {
    sockets: RwLock<SocketTable<N>>,
}

impl<N: Net> Tcp<N> {
    pub fn new(net: Arc<N>) -> Self {
        let sockets = RwLock::new(SocketTable::new(net));
        Tcp { sockets }
    }

    /// Attempts to connect to a host, establishing the client side of a TCP connection.
    pub async fn connect(&self, remote: Remote) -> Result<TcpConn, TcpConnError> {
        let mut sockets = self.sockets.write().await;
        let socket = sockets.add_new_socket(remote).map_err(|e| match e {
            AddSocketError::ConnectionExists(sid) => TcpConnError::ConnectionExists(sid.remote()),
        })?;

        let socket_id = socket.id();
        let on_connected = socket
            .initiate_connection()
            .await
            .expect("Failed to send SYN packet");
        drop(sockets);

        match on_connected
            .await
            .expect("Failed to receive connection status")
        {
            Ok(r) => Ok(r),
            Err(e) => {
                self.sockets.write().await.remove_by_id(socket_id);
                Err(e)
            }
        }
    }

    /// Starts listening for incoming connections at a port. Opens a listener socket.
    pub async fn listen(&self, port: Port) -> Result<TcpListener, TcpListenError> {
        let mut sockets = self.sockets.write().await;
        let socket = sockets.add_new_listen_socket(port).map_err(|e| match e {
            AddSocketError::ConnectionExists(sid) => TcpListenError::PortOccupied(sid.local_port()),
        })?;
        Ok(socket.listen(port).await.unwrap())
    }

    pub async fn send_on_socket_descriptor(
        &self,
        socket_descriptor: SocketDescriptor,
        payload: &[u8],
    ) -> Result<(), TcpSendError> {
        let sockets = self.sockets.read().await;
        let socket = sockets
            .get_socket_by_descriptor(socket_descriptor)
            .ok_or(TcpSendError::NoSocket(socket_descriptor))?;

        socket.send_all(payload).await
    }

    pub async fn read_on_socket_descriptor(
        &self,
        socket_descriptor: SocketDescriptor,
        n_bytes: usize,
    ) -> Result<Vec<u8>, TcpReadError> {
        let sockets = self.sockets.read().await;
        let socket = sockets
            .get_socket_by_descriptor(socket_descriptor)
            .ok_or(TcpReadError::NoSocket(socket_descriptor))?;

        if socket
            .is_read_closed()
            .await
            .ok_or(TcpReadError::ConnNotEstablished)?
        {
            return Err(TcpReadError::Closed(0));
        }

        let mut out_buf = vec![0; n_bytes];
        socket.read_all(&mut out_buf).await?;

        Ok(out_buf)
    }

    pub async fn get_socket(&self, socket_id: SocketId) -> Option<SocketRef<'_, N>> {
        let table = self.sockets.read().await;
        let socket: *const Socket<N> = table.socket_map.get(&socket_id)?;
        Some(SocketRef {
            _guard: table,
            socket,
        })
    }

    pub async fn get_socket_by_descriptor(
        &self,
        socket_descriptor: SocketDescriptor,
    ) -> Option<SocketRef<'_, N>> {
        let table = self.sockets.read().await;
        let id = *table.socket_id_map.get(&socket_descriptor)?;
        let socket: *const Socket<N> = table.socket_map.get(&id)?;
        Some(SocketRef {
            _guard: table,
            socket,
        })
    }

    pub async fn get_socket_descriptor(&self, socket_id: SocketId) -> Option<SocketDescriptor> {
        let table = self.sockets.read().await;
        table.socket_map.get(&socket_id).map(|s| s.descriptor())
    }

    pub async fn close(&self, socket_id: SocketId) -> Result<(), TcpCloseError> {
        let table = self.sockets.read().await;
        let sock = table
            .get_socket_by_id(socket_id)
            .ok_or(TcpCloseError::NoSocketOnId(socket_id))?;

        sock.close().await;
        Ok(())
    }

    pub async fn close_by_descriptor(
        &self,
        socket_descriptor: SocketDescriptor,
    ) -> Result<(), TcpCloseError> {
        let table = self.sockets.read().await;
        let sock = table
            .get_socket_by_descriptor(socket_descriptor)
            .ok_or(TcpCloseError::NoSocketOnDescriptor(socket_descriptor))?;

        if matches!(sock.status().await, SocketStatus::Listen) {
            // For listen sockets, delete directly
            let sock_id = sock.id();
            drop(table);
            self.sockets.write().await.remove_by_id(sock_id);
        } else {
            sock.close().await;
        }

        Ok(())
    }

    pub async fn print_sockets(&self, file: Option<String>) {
        match file {
            Some(file) => {
                let mut f = File::create(file).await.unwrap();
                f.write_all(b"id\tstate\tlocal window size\tremote window size\n")
                    .await
                    .unwrap();
                let table = self.sockets.read().await;
                for (_, socket) in table.socket_map.iter() {
                    f.write_all(socket.as_table_entry_string().await.as_bytes())
                        .await
                        .unwrap();
                }
            }
            None => {
                println!("id\tstate\t\tlocal window size\tremote window size");
                let table = self.sockets.read().await;
                for (_, socket) in table.socket_map.iter() {
                    println!("{}", socket.as_table_entry_string().await);
                }
            }
        }
    }
}

pub struct SocketRef<'a, N: Net + 'static> {
    _guard: RwLockReadGuard<'a, SocketTable<N>>,
    socket: *const Socket<N>,
}

impl<'a, N: Net + 'static> Deref for SocketRef<'a, N> {
    type Target = Socket<N>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: this socket pointer is valid because this struct holds a
        // read guard to the socket table, where this socket resides.
        unsafe { &*self.socket }
    }
}

// SAFETY: this can be sent across threads because RwLockReadGuard is Send.
unsafe impl<'a, N: Net> Send for SocketRef<'a, N> {}

#[derive(Debug)]
pub enum AddSocketError {
    ConnectionExists(SocketId),
}

pub(crate) struct SocketTable<N: Net + 'static> {
    socket_id_map: HashMap<SocketDescriptor, SocketId>,
    socket_map: HashMap<SocketId, Socket<N>>,
    socket_builder: SocketBuilder<N>,
}

impl<N: Net> SocketTable<N> {
    pub fn new(net: Arc<N>) -> Self {
        Self {
            socket_builder: SocketBuilder::new(net),
            socket_id_map: HashMap::new(),
            socket_map: HashMap::new(),
        }
    }

    pub fn add_new_socket(&mut self, remote: Remote) -> Result<&mut Socket<N>, AddSocketError> {
        let sock_id = self.socket_builder.make_socket_id(remote);
        let (descriptor, socket) = self.socket_builder.build_with_id(sock_id);

        self.insert(descriptor, socket)
    }

    pub fn add_new_listen_socket(
        &mut self,
        local_port: Port,
    ) -> Result<&mut Socket<N>, AddSocketError> {
        let (descriptor, socket) = self
            .socket_builder
            .build_with_id(SocketId::for_listen_socket(local_port));

        self.insert(descriptor, socket)
    }

    pub fn add_new_syn_recvd_socket(
        &mut self,
        remote: Remote,
        local_port: Port,
        syn_recvd_state: SynReceived<N>,
    ) -> Result<&mut Socket<N>, AddSocketError> {
        let sock_id = SocketId::build()
            .with_remote_ip(remote.ip())
            .with_remote_port(remote.port())
            .with_local_port(local_port)
            .build()
            .unwrap();

        let descriptor = self.socket_builder.allocate_socket_descriptor();
        let s = syn_recvd_state.into_socket(sock_id, descriptor);

        self.insert(descriptor, s)
    }

    pub fn remove_by_id(&mut self, id: SocketId) {
        // TODO: lazily delete socket entries in socket_id_map
        self.socket_map.remove(&id);
    }

    pub fn get_socket_by_id(&self, id: SocketId) -> Option<&Socket<N>> {
        self.socket_map.get(&id)
    }

    pub fn get_socket_by_descriptor(&self, descriptor: SocketDescriptor) -> Option<&Socket<N>> {
        self.socket_id_map
            .get(&descriptor)
            .and_then(|sock_id| self.socket_map.get(sock_id))
    }

    pub fn get_listener_socket(&self, port: Port) -> Option<&Socket<N>> {
        let id = SocketId::for_listen_socket(port);
        self.get_socket_by_id(id)
    }

    fn insert(
        &mut self,
        descriptor: SocketDescriptor,
        socket: Socket<N>,
    ) -> Result<&mut Socket<N>, AddSocketError> {
        let socket_id = socket.id();

        let sock_ref = self
            .socket_map
            .try_insert(socket_id, socket)
            .map_err(|_| AddSocketError::ConnectionExists(socket_id))?;

        self.socket_id_map
            .try_insert(descriptor, socket_id)
            .expect("Found duplicate socket descriptor");

        Ok(sock_ref)
    }
}

struct SocketBuilder<N> {
    next_socket_descriptor: usize,
    next_port: u16,
    net: Arc<N>,
}

impl<N: Net> SocketBuilder<N> {
    fn new(net: Arc<N>) -> Self {
        Self {
            net,
            next_port: 1024,
            next_socket_descriptor: 0,
        }
    }

    fn build_with_id(&mut self, socket_id: SocketId) -> (SocketDescriptor, Socket<N>) {
        let descriptor = self.allocate_socket_descriptor();
        let sock = Socket::new(socket_id, descriptor, self.net.clone());
        (descriptor, sock)
    }

    fn make_socket_id(&mut self, remote: Remote) -> SocketId {
        let local_port = Port(self.next_port);
        self.next_port += 1;
        SocketId::build()
            .with_remote_ip(remote.ip())
            .with_remote_port(remote.port())
            .with_local_port(local_port)
            .build()
            .unwrap()
    }

    fn allocate_socket_descriptor(&mut self) -> SocketDescriptor {
        let descriptor = SocketDescriptor(
            self.next_socket_descriptor
                .try_into()
                .expect("Socket descriptor overflow"),
        );
        self.next_socket_descriptor += 1;
        descriptor
    }
}

pub struct TcpHandler<N: Net + 'static> {
    tcp: Arc<Tcp<N>>,
}

impl<N: Net> TcpHandler<N> {
    pub fn new(tcp: Arc<Tcp<N>>) -> Self {
        Self { tcp }
    }
}

#[async_trait]
impl<N: Net, DP: DropPolicy> ProtocolHandler<DP> for TcpHandler<N> {
    async fn handle_packet<'a>(
        &self,
        ip_header: &Ipv4HeaderSlice<'a>,
        payload: &[u8],
        _net: &VtLinkNet<DP>,
    ) where
        DP: DropPolicy,
    {
        // Step 1: validate checksum
        let tcp_header = TcpHeaderSlice::from_slice(payload).expect("Failed to parse TCP Header");
        log::debug!(
            "Received packet tcp header len: {}, source: {}:{}, dest: {}:{}",
            payload.len(),
            ip_header.source_addr(),
            tcp_header.source_port(),
            ip_header.destination_addr(),
            tcp_header.destination_port()
        );

        let sock_id = SocketId::build()
            .with_remote_ip(ip_header.source_addr())
            .with_remote_port(tcp_header.source_port().into())
            .with_local_port(tcp_header.destination_port().into())
            .build()
            .unwrap();
        let checksum = tcp_header.checksum();

        let tcp_payload = &payload[tcp_header.slice().len()..];
        if checksum
            != tcp_header
                .calc_checksum_ipv4(ip_header, tcp_payload)
                .unwrap()
        {
            log::error!("TCP checksum failed");
        } else {
            let sockets = self.tcp.sockets.read().await;
            let action = match sockets.get_socket_by_id(sock_id) {
                Some(socket) => {
                    socket
                        .handle_packet(ip_header, &tcp_header, tcp_payload)
                        .await
                }
                None => match sockets.get_listener_socket(tcp_header.destination_port().into()) {
                    Some(listener_sock) => {
                        listener_sock
                            .handle_packet(ip_header, &tcp_header, payload)
                            .await
                    }
                    None => {
                        log::info!("Received TCP packet that doesn't match with any connection");
                        return;
                    }
                },
            };

            if let Some(action) = action {
                match action {
                    UpdateAction::NewSynReceivedSocket(syn_recvd) => {
                        drop(sockets);
                        self.tcp
                            .sockets
                            .write()
                            .await
                            .add_new_syn_recvd_socket(
                                Remote::new(
                                    ip_header.source_addr(),
                                    tcp_header.source_port().into(),
                                ),
                                tcp_header.destination_port().into(),
                                syn_recvd,
                            )
                            .unwrap();
                    }
                    UpdateAction::CloseSocket(id) => {
                        drop(sockets);
                        self.tcp.sockets.write().await.remove_by_id(id);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{future::Future, sync::Arc, time::Duration};

    use tokio::sync::Barrier;

    use crate::{
        drop_policy::{DropFactor, NeverDrop},
        node::{Node, NodeBuilder},
        protocol::{rip::RipHandler, tcp::socket::SocketStatus, Protocol},
        Args,
    };

    const NUM_REPEATS: usize = 1;

    #[tokio::test]
    async fn hello_world() {
        // A minimal test case that establishes TCP connection and sends some bytes.

        for _ in 0..NUM_REPEATS {
            let payload = String::from("hello world!").as_bytes().into();
            let f = test_send_recv(payload, vec![], NeverDrop, NeverDrop);
            test_timeout(Duration::from_secs(1), f).await;
        }
    }

    #[tokio::test]
    async fn send_file() {
        let test_file_size = 1_000_000;

        for _ in 0..NUM_REPEATS {
            let f = test_send_file(make_in_mem_test_file(test_file_size), NeverDrop);
            test_timeout(Duration::from_secs(8), f).await;
        }
    }

    #[tokio::test]
    async fn lossy_send_file() {
        let test_file_size = 1_500_000;

        for _ in 0..NUM_REPEATS {
            let f = test_send_recv(
                make_in_mem_test_file(test_file_size),
                vec![],
                NeverDrop,
                DropFactor::new(0.05),
            );
            test_timeout(Duration::from_secs(10), f).await;
        }
    }

    #[tokio::test]
    async fn bidirectional_send_file() {
        let test_file_size = 10_000_000;

        for _ in 0..NUM_REPEATS {
            let f = test_send_recv(
                make_in_mem_test_file(test_file_size),
                make_in_mem_test_file(test_file_size),
                NeverDrop,
                NeverDrop,
            );
            test_timeout(Duration::from_secs(10), f).await;
        }
    }

    #[tokio::test]
    async fn lossy_bidirectional_send_file() {
        let test_file_size = 2_000_000;

        for _ in 0..NUM_REPEATS {
            let f = test_send_recv(
                make_in_mem_test_file(test_file_size),
                make_in_mem_test_file(test_file_size),
                DropFactor::new(0.05),
                DropFactor::new(0.05),
            );
            test_timeout(Duration::from_secs(10), f).await;
        }
    }

    #[tokio::test]
    async fn close_conn() {
        let payload: Vec<_> = "hello world!".as_bytes().into();
        let payload_clone = payload.clone();

        let abc_net = crate::fixture::netlinks::abc::gen_unique();
        let send_cfg = abc_net.a.clone();
        let recv_cfg = abc_net.b.clone();

        let recv_listen_port = Port(5656);
        let listen_barr = Arc::new(Barrier::new(2));
        let listen_barr_clone = listen_barr.clone();
        let close_barr = Arc::new(Barrier::new(2));
        let close_barr_clone = close_barr.clone();

        let n1_cfg = send_cfg.clone();
        let n2_cfg = recv_cfg.clone();

        let n1 = tokio::spawn(async move {
            let node = create_and_start_node(n1_cfg, NeverDrop).await;
            listen_barr_clone.wait().await;

            let dest_ip = {
                let recv_ips = n2_cfg.get_my_interface_ips();
                recv_ips[0]
            };

            let conn = node.connect(dest_ip, recv_listen_port).await.unwrap();
            conn.send_all(&payload).await.unwrap();

            let socket_id = conn.socket_id();
            node.close_socket(socket_id).await.unwrap();

            // test closed socket cannot be written into
            let r = conn.send_all(&payload).await;
            assert_eq!(r.unwrap_err(), TcpSendError::ConnClosed);

            // Give socket state some time to settle.
            tokio::time::sleep(Duration::from_secs(2)).await;
            {
                let sock_ref = node.get_socket(socket_id).await.unwrap();
                assert_eq!(sock_ref.status().await, SocketStatus::FinWait2);
            }
            close_barr.wait().await;

            // test closed socket can still receive data
            let mut buf = vec![0; payload.len()];
            conn.read_all(&mut buf).await.unwrap();
            assert!(buf == payload);
        });

        let n2 = tokio::spawn(async move {
            let node = create_and_start_node(recv_cfg, NeverDrop).await;

            let mut listener = node.listen(recv_listen_port).await.unwrap();
            listen_barr.wait().await;

            let conn = listener.accept().await.unwrap();

            let mut buf = vec![0; payload_clone.len()];
            conn.read_all(&mut buf).await.unwrap();
            assert!(buf == payload_clone);

            close_barr_clone.wait().await;
            let socket_id = conn.socket_id();
            // Remote should be in passvie close
            {
                let sock_ref = node.get_socket(socket_id).await.unwrap();
                assert_eq!(sock_ref.status().await, SocketStatus::CloseWait);
            }
            conn.send_all(&payload_clone).await.unwrap();
        });

        n1.await.unwrap();
        n2.await.unwrap();
    }

    async fn test_send_file(in_mem_file: Vec<u8>, drop_policy: impl DropPolicy) {
        let abc_net = crate::fixture::netlinks::abc::gen_unique();
        let send_cfg = abc_net.a.clone();
        let recv_cfg = abc_net.b.clone();

        let n1_cfg = send_cfg.clone();
        let n2_cfg = recv_cfg.clone();
        let expected = in_mem_file.clone();
        let listen_port = Port(8981);

        let n1 = tokio::spawn(async move {
            let node = create_and_start_node(n1_cfg, NeverDrop).await;

            let dest_ip = {
                let recv_ips = n2_cfg.get_my_interface_ips();
                recv_ips[0]
            };
            let remote = Remote::new(dest_ip, listen_port);

            // Give listener time to set up
            tokio::time::sleep(Duration::from_secs(1)).await;
            node.connect_and_send_bytes(remote, &in_mem_file)
                .await
                .unwrap();
        });

        let n2 = tokio::spawn(async move {
            let node = create_and_start_node(recv_cfg, drop_policy).await;
            let got = node.listen_and_recv_bytes(listen_port).await.unwrap();
            assert_eq!(got, expected);
        });

        n1.await.unwrap();
        n2.await.unwrap();
    }

    // General-purposed TCP test that sends two payloads to one another.
    async fn test_send_recv(
        payload1: Vec<u8>,
        payload2: Vec<u8>,
        n1_drop_policy: impl DropPolicy,
        n2_drop_policy: impl DropPolicy,
    ) {
        let abc_net = crate::fixture::netlinks::abc::gen_unique();
        let send_cfg = abc_net.a.clone();
        let recv_cfg = abc_net.b.clone();

        let recv_listen_port = Port(5656);
        let payload1_clone = payload1.clone();
        let payload2_clone = payload2.clone();
        let barr = Arc::new(Barrier::new(2));

        let listen_barr = barr.clone();
        let n1_cfg = send_cfg.clone();
        let n2_cfg = recv_cfg.clone();

        let n1 = tokio::spawn(async move {
            let node = create_and_start_node(n1_cfg, n1_drop_policy).await;
            listen_barr.wait().await;

            let dest_ip = {
                let recv_ips = n2_cfg.get_my_interface_ips();
                recv_ips[0]
            };
            let conn = node.connect(dest_ip, recv_listen_port).await.unwrap();

            let conn2 = conn.clone();
            let snd = tokio::spawn(async move {
                conn2.send_all(&payload1).await.unwrap();
            });

            let rcv = tokio::spawn(async move {
                let mut buf = vec![0; payload2_clone.len()];
                conn.read_all(&mut buf).await.unwrap();
                assert!(buf == payload2_clone);
            });

            snd.await.unwrap();
            rcv.await.unwrap();
        });

        let n2 = tokio::spawn(async move {
            let node = create_and_start_node(recv_cfg, n2_drop_policy).await;

            let mut listener = node.listen(recv_listen_port).await.unwrap();
            barr.wait().await;

            let conn = listener.accept().await.unwrap();

            let conn2 = conn.clone();
            let rcv = tokio::spawn(async move {
                let mut buf = vec![0; payload1_clone.len()];
                conn2.read_all(&mut buf).await.unwrap();
                assert!(buf == payload1_clone);
            });
            let snd = tokio::spawn(async move {
                conn.send_all(&payload2).await.unwrap();
            });

            snd.await.unwrap();
            rcv.await.unwrap();
        });

        n1.await.unwrap();
        n2.await.unwrap();
    }

    fn make_in_mem_test_file(size: usize) -> Vec<u8> {
        let base_data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        base_data.into_iter().cycle().take(size).collect()
    }

    async fn create_and_start_node<DP: DropPolicy>(cfg: Args, drop_policy: DP) -> Arc<Node<DP>> {
        let node = Arc::new(
            NodeBuilder::new(&cfg)
                .with_rip_interval(Duration::from_millis(1))
                .with_entry_max_age(Duration::from_millis(12))
                .with_prune_interval(Duration::from_millis(1))
                .with_drop_policy(drop_policy)
                .with_protocol_handler(Protocol::Rip, RipHandler::default())
                .build()
                .await,
        );
        let node_runner = node.clone();
        tokio::spawn(async move {
            node_runner.run().await;
        });
        // Give nodes time to converge on routes
        tokio::time::sleep(Duration::from_millis(300)).await;
        node
    }

    async fn test_timeout<F: Future>(dur: Duration, f: F) {
        tokio::time::timeout(dur, f)
            .await
            .expect("Test should finish within time limit");
    }
}
