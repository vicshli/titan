use crate::protocol::tcp::{TcpAcceptError, TcpListenError, TcpReadError, TcpSendError};
use crate::protocol::Protocol;
use crate::route::{Router, SendError};
use async_trait::async_trait;
use etherparse::{Ipv4HeaderSlice, Ipv6RoutingExtensions, TcpHeader, TcpHeaderSlice};
use rand::{random, thread_rng, Rng};
use replace_with::replace_with_or_abort;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::oneshot;

use super::{Port, SocketId, TCP_DEFAULT_WINDOW_SZ};

#[derive(Copy, Clone)]
pub struct TcpConn {
    // sendBuf: SendBuf<n>,
    // recvBuf: RecvBuf<n>,
}

#[derive(Copy, Clone)]
pub struct TcpListener {
    port: u16,
}

pub struct TcpMessage {
    header: TcpHeader,
    payload: Vec<u8>,
}

impl TcpConn {
    /// Sends bytes over a connection.
    ///
    /// Blocks until all bytes have been acknowledged by the other end.
    pub async fn send_all(&self, bytes: &[u8]) -> Result<(), TcpSendError> {
        todo!()
    }

    /// Reads N bytes from the connection, where N is `out_buffer`'s size.
    pub async fn read_all(&self, out_buffer: &mut [u8]) -> Result<(), TcpReadError> {
        todo!()
    }
}

impl TcpListener {
    /// Creates a new TcpListener.
    ///
    /// The listener can be used to accept incoming connections
    pub fn new(port: u16) -> Self {
        Self { port }
    }
    /// Yields new client connections.
    ///
    /// To repeatedly accept new client connections:
    /// ```
    /// while let Ok(conn) = listener.accept().await {
    ///     // handle new conn...
    /// }
    /// ```
    pub async fn accept(&self) -> Result<TcpConn, TcpAcceptError> {
        // TODO: create a new Tcp socket and state machine. (Keep the listener
        // socket, open a new socket to handle this client).
        //

        // 1. The new Tcp state machine should transition to SYN_RECVD after
        // replying syn+ack to client.
        // 2. When Tcp handler receives client's ack packet (3rd step in
        // handshake), the new Tcp state machine should transition to ESTABLISHED.
        todo!()
    }
}

#[derive(Debug)]
pub enum TransportError {
    DestUnreachable(Ipv4Addr),
}

pub struct Socket<const N: usize> {
    id: SocketId,
    port: Port,
    pub state: Option<TcpState>,
    pub sender: oneshot::Sender<()>,
    pub receiver: Option<oneshot::Receiver<()>>,
    router: Arc<Router>,
}

pub enum TcpState {
    Closed(Closed),
    SynSent(SynSent),
    SynReceived(SynReceived),
    Established(Established),
    // TODO: add more state variants
}

impl TcpState {
    fn new(router: Arc<Router>) -> Self {
        Self::Closed(Closed::new(router))
    }
}

impl From<Closed> for TcpState {
    fn from(s: Closed) -> Self {
        Self::Closed(s)
    }
}

impl From<SynSent> for TcpState {
    fn from(s: SynSent) -> Self {
        Self::SynSent(s)
    }
}

impl From<SynReceived> for TcpState {
    fn from(s: SynReceived) -> Self {
        Self::SynReceived(s)
    }
}

impl From<Established> for TcpState {
    fn from(s: Established) -> Self {
        Self::Established(s)
    }
}

pub struct Closed {
    seq_no: u32,
    router: Arc<Router>,
}

impl Closed {
    pub fn new(router: Arc<Router>) -> Self {
        Self {
            router,
            seq_no: Self::gen_rand_seq_no(),
        }
    }

    pub async fn connect(
        self,
        src_port: Port,
        dest: (Ipv4Addr, Port),
    ) -> Result<SynSent, TransportError> {
        let (dest_ip, dest_port) = dest;

        let syn_pkt = self.make_syn_packet(src_port, dest_port);
        self.router
            .send(&syn_pkt, Protocol::Tcp, dest_ip)
            .await
            .map_err(|_| TransportError::DestUnreachable(dest_ip))?;

        Ok(SynSent {
            src_port,
            dest_port,
            dest_ip,
            router: self.router,
            seq_no: self.seq_no,
        })
    }

    pub async fn listen(self, port: Port) -> Listen {
        Listen {
            port,
            seq_no: self.seq_no,
            router: self.router,
        }
    }

    fn make_syn_packet(&self, src_port: Port, dest_port: Port) -> Vec<u8> {
        let mut bytes = Vec::new();

        let header = TcpHeader::new(
            src_port.0,
            dest_port.0,
            self.seq_no,
            TCP_DEFAULT_WINDOW_SZ.try_into().unwrap(),
        );
        header.syn = true;
        header.write(&mut bytes).unwrap();

        bytes
    }

    fn gen_rand_seq_no() -> u32 {
        thread_rng().gen_range(0..u16::MAX).into()
    }
}

pub struct Listen {
    port: Port,
    seq_no: u32,
    router: Arc<Router>,
}

impl Listen {
    pub async fn handle_connection_request<'a>(
        &self,
        ip_header: Ipv4HeaderSlice<'a>,
        syn_packet: TcpHeaderSlice<'a>,
    ) -> Result<SynReceived, TransportError> {
        assert!(syn_packet.syn());

        let reply_ip = ip_header.source_addr();

        let ack_pkt = self.make_syn_ack_packet(&syn_packet);

        self.router
            .send(&ack_pkt, Protocol::Tcp, reply_ip)
            .await
            .map_err(|_| TransportError::DestUnreachable(reply_ip))?;

        Ok(SynReceived {
            seq_no: self.seq_no,
            src_port: self.port,
            dest_ip: ip_header.source_addr(),
            dest_port: Port(syn_packet.source_port()),
            router: self.router.clone(),
        })
    }

    fn make_syn_ack_packet<'a>(&self, syn_packet: &TcpHeaderSlice<'a>) -> Vec<u8> {
        let mut bytes = Vec::new();

        let src_port = self.port.0;
        let dst_port = syn_packet.source_port();

        let header = TcpHeader::new(
            src_port,
            dst_port,
            self.seq_no,
            TCP_DEFAULT_WINDOW_SZ.try_into().unwrap(),
        );
        header.syn = true;
        header.ack = true;
        header.acknowledgment_number = syn_packet.acknowledgment_number() + 1;

        header.write(&mut bytes).unwrap();

        bytes
    }
}

pub struct SynSent {
    seq_no: u32,
    src_port: Port,
    dest_ip: Ipv4Addr,
    dest_port: Port,
    router: Arc<Router>,
}

impl SynSent {
    pub async fn establish<'a>(
        mut self,
        syn_ack_packet: &TcpHeaderSlice<'a>,
    ) -> Result<Established, TransportError> {
        assert!(syn_ack_packet.ack());
        let ack_pkt = self.make_ack_packet(syn_ack_packet);

        self.router
            .send(&ack_pkt, Protocol::Tcp, self.dest_ip)
            .await
            .map_err(|_| TransportError::DestUnreachable(self.dest_ip))?;

        Ok(Established {
            seq_no: self.seq_no,
            src_port: self.src_port,
            dest_ip: self.dest_ip,
            dest_port: self.dest_port,
            router: self.router,
            last_ack_no: syn_ack_packet.acknowledgment_number(),
        })
    }

    fn make_ack_packet<'a>(&mut self, syn_ack_packet: &TcpHeaderSlice<'a>) -> Vec<u8> {
        let mut bytes = Vec::new();

        let seq_no = {
            self.seq_no += 1;
            self.seq_no
        };

        let header = TcpHeader::new(
            self.src_port.0,
            self.dest_port.0,
            seq_no,
            TCP_DEFAULT_WINDOW_SZ.try_into().unwrap(),
        );
        header.syn = true;
        header.ack = true;
        header.acknowledgment_number = syn_ack_packet.acknowledgment_number() + 1;

        header.write(&mut bytes).unwrap();

        bytes
    }
}

pub struct SynReceived {
    seq_no: u32,
    src_port: Port,
    dest_ip: Ipv4Addr,
    dest_port: Port,
    router: Arc<Router>,
}

impl SynReceived {
    pub async fn establish<'a>(self, ack_packet: &TcpHeaderSlice<'a>) -> Established {
        assert!(ack_packet.ack());

        Established {
            seq_no: self.seq_no,
            src_port: self.src_port,
            dest_ip: self.dest_ip,
            dest_port: self.dest_port,
            router: self.router,
            last_ack_no: ack_packet.acknowledgment_number(),
        }
    }
}

pub struct Established {
    seq_no: u32,
    src_port: Port,
    dest_ip: Ipv4Addr,
    dest_port: Port,
    router: Arc<Router>,
    last_ack_no: u32,
    // TODO:
    // conn: TcpConn,
}

impl<const N: usize> Socket<N> {
    pub fn new(id: SocketId, port: Port, router: Arc<Router>) -> Self {
        let (sender, receiver) = oneshot::channel();
        Self {
            id,
            port,
            state: Some(TcpState::new(router.clone())),
            sender,
            receiver: Some(receiver),
            router,
        }
    }

    pub fn id(&self) -> SocketId {
        self.id
    }

    pub async fn connect(
        &mut self,
        dst_addr: Ipv4Addr,
        dst_port: Port,
    ) -> Result<(), TcpSendError> {
        if let Some(s) = state {
            self.state = s;
        }
        Ok(())
    }

    pub async fn handle_packet<'a>(
        &mut self,
        ip_header: &Ipv4HeaderSlice<'a>,
        tcp_header: &TcpHeaderSlice<'a>,
        payload: &[u8],
    ) {
        let state = self
            .state
            .take()
            .expect("A socket should not handle packets concurrently");

        self.state = Some(match state {
            TcpState::Closed(s) => {
                panic!("Should not receive packet under closed state");
            }
            TcpState::SynSent(s) => s.establish(tcp_header).await.unwrap().into(),
            TcpState::SynReceived(s) => s.establish(tcp_header).await.into(),
            TcpState::Established(_) => {
                todo!()
            }
        });
    }
}
