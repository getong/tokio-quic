//! Async QUIC Listener/Socket for [tokio](https://tokio.rs/) using [quiche](https://github.com/cloudflare/quiche).
//!
//! ### Examples
//!
//! #### [Client](https://github.com/cauvmou/tokio-quic/blob/main/examples/client.rs)
//!
//! First create a `QuicSocket`.
//! ```rust,ignore
//! let mut connection = QuicSocket::bind("127.0.0.1:0")
//!         .await?
//!         .connect(Some("localhost"), "127.0.0.1:4433")
//!         .await?;
//! ```
//! Then you can start opening new `QuicStream`s or receive incoming ones from the server.
//! ```rust,ignore
//! let mut stream = connection.bidi(1).await?;
//! ```
//! ```rust,ignore
//! let mut stream = connection.incoming().await?;
//! ```
//! These implement the tokio `AsyncRead` and `AsyncWrite` traits.
//!
//! #### [Server](https://github.com/cauvmou/tokio-quic/blob/main/examples/server.rs)
//!
//! Again create a `QuicListener`.
//!
//! ```rust,ignore
//! let mut listener = QuicListener::bind("127.0.0.1:4433").await?;
//! ```
//! Then you can use a while loop to accept incoming connection and either handle them directly on the thread or move them to a new one.
//! ```rust,ignore
//! while let Ok(mut connection) = listener.accept().await {
//!     tokio::spawn(async move {
//!         let mut stream = connection.incoming().await?;
//!         ...
//!         stream.shutdown().await?;
//!     });
//! }
//! ```

use log::trace;
use std::sync::Arc;

use crate::backend::Handshaker;
use backend::{
    client,
    manager::{self, Manager},
    server,
    timer::Timer,
};
use config::{MAX_DATAGRAM_SIZE, STREAM_BUFFER_SIZE};
use connection::{QuicConnection, ToClient, ToServer};
use error::Result;
use quiche::ConnectionId;
use rand::Rng;
use ring::rand::SystemRandom;
use tokio::{
    net::{ToSocketAddrs, UdpSocket},
    sync::mpsc::{self, UnboundedReceiver},
    task::JoinHandle,
};

mod backend;
pub mod config;
pub mod connection;
mod crypto;
pub mod error;
pub mod stream;
mod io;
mod async_io;

pub use io::{TryRead, TryWrite};

#[derive(Debug)]
/// Passed between the backend and a stream for exchange of data.
pub(crate) enum Message {
    Data {
        stream_id: u64,
        bytes: Vec<u8>,
        fin: bool,
    },
    /// Contains the id of the stream to be closed
    Close(u64),
}

/// `QuicListener` is used to bind to a specified address/port.
///
/// It can be configured using a `quiche::Config` struct.
/// A base config can be obtained from `tokio_quic::config::default()`.
///
/// If the feature `key-gen` is enabled this config will already come with a certificate and private key,
/// although these are just for testing and are not recommended to be used in production.
pub struct QuicListener {
    io: Arc<UdpSocket>,
    #[allow(unused)]
    handle: JoinHandle<Result<()>>,
    connection_recv: UnboundedReceiver<manager::Client>,
}

impl QuicListener {
    #[cfg(not(feature = "key-gen"))]
    pub async fn bind<A: ToSocketAddrs>(
        addr: A,
        key_pem: &str,
        cert_pem: &str,
        secret: Vec<u8>,
    ) -> Result<Self> {
        let mut config = config::default();
        config.load_priv_key_from_pem_file(key_pem).unwrap();
        config.load_cert_chain_from_pem_file(cert_pem).unwrap();
        Self::bind_with_config(addr, config, secret).await
    }

    #[cfg(feature = "key-gen")]
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        use ring::rand::Random;
        let rng = SystemRandom::new();
        let random: Random<[u8; 16]> = ring::rand::generate(&rng).unwrap();
        Self::bind_with_config(addr, config::default(), random.expose().to_vec()).await
    }

    pub async fn bind_with_config<A: ToSocketAddrs>(
        addr: A,
        config: quiche::Config,
        secret: Vec<u8>,
    ) -> Result<Self> {
        trace!("Bind listener [{secret:?}]");
        let io = Arc::new(UdpSocket::bind(addr).await?);
        let rng = SystemRandom::new();
        let (tx, connection_recv) = mpsc::unbounded_channel();
        let manager = Manager::new(
            io.clone(),
            ring::hmac::Key::generate(ring::hmac::HMAC_SHA256, &rng).unwrap(),
            secret,
            config,
            tx,
        );
        let handle = tokio::spawn(manager);
        Ok(Self {
            io,
            handle,
            connection_recv,
        })
    }

    /// Accepts an incoming connection.
    pub async fn accept(&mut self) -> Result<QuicConnection<ToClient>> {
        let manager::Client { connection, recv } = self.connection_recv.recv().await.unwrap();

        let mut inner = server::Inner {
            io: self.io.clone(),
            connection,
            data_recv: recv,
            send_flush: false,
            send_end: 0,
            send_pos: 0,
            recv_buf: vec![0; STREAM_BUFFER_SIZE],
            send_buf: vec![0; MAX_DATAGRAM_SIZE],
            timer: Timer::Unset,
            last_address: None,
        };
        trace!(
            "Accepted connection trace-id: {:?}, server-name: {:?}",
            inner.connection.trace_id(),
            inner.connection.server_name()
        );
        Handshaker(&mut inner).await?;
        trace!(
            "Handshake complete trace-id: {:?}, server-name: {:?}",
            inner.connection.trace_id(),
            inner.connection.server_name()
        );
        Ok(QuicConnection::<ToClient>::new(inner))
    }
}

/// `QuicSocket` opens a connection from a specified address/port to a server.
///
/// It can be configured using a `quiche::Config` struct.
/// A base config can be obtained from `tokio_quic::config::default()`.
///
/// If the feature `key-gen` is enabled this config will already come with a certificate and private key,
/// although these are just for testing and are not recommended to be used in production.
pub struct QuicSocket {
    io: Arc<UdpSocket>,
    config: quiche::Config,
}

impl QuicSocket {
    #[cfg(not(feature = "key-gen"))]
    /// Bind to a specified address.
    pub async fn bind<A: ToSocketAddrs>(addr: A, key_pem: &str, cert_pem: &str) -> Result<Self> {
        let mut config = config::default();
        config.load_priv_key_from_pem_file(key_pem).unwrap();
        config.load_cert_chain_from_pem_file(cert_pem).unwrap();
        Self::bind_with_config(addr, config).await
    }

    #[cfg(feature = "key-gen")]
    /// Bind to a specified address.
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        Self::bind_with_config(addr, config::default()).await
    }

    /// Bind to a specified address with a `quiche::Config`.
    pub async fn bind_with_config<A: ToSocketAddrs>(
        addr: A,
        config: quiche::Config,
    ) -> Result<Self> {
        Ok(Self {
            io: Arc::new(UdpSocket::bind(addr).await?),
            config,
        })
    }

    /// Connect to a remote server.
    ///
    /// `server_name` needs to have a value in order to validate the server's certificate.
    /// Can be set to `None`, if validation is turned off.
    pub async fn connect<A: ToSocketAddrs>(
        &mut self,
        server_name: Option<&str>,
        addr: A,
    ) -> Result<QuicConnection<ToServer>> {
        self.io.connect(addr).await?;
        let mut scid = vec![0; 16];
        rand::thread_rng().fill(&mut *scid);
        let scid: ConnectionId = scid.into();
        let connection = quiche::connect(
            server_name,
            &scid,
            self.io.local_addr()?,
            self.io.peer_addr()?,
            &mut self.config,
        )
        .unwrap();

        let mut inner = client::Inner {
            io: self.io.clone(),
            connection,
            send_flush: false,
            send_end: 0,
            send_pos: 0,
            recv_buf: vec![0; STREAM_BUFFER_SIZE],
            send_buf: vec![0; MAX_DATAGRAM_SIZE],
            timer: Timer::Unset,
        };

        Handshaker(&mut inner).await?;

        Ok(QuicConnection::<ToServer>::new(inner))
    }
}
